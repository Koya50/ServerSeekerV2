use chrono::Utc;
use sqlx::types::ipnet::{IpNet, Ipv4Net};
use std::{net::Ipv4Addr, str::FromStr};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tracing::{debug, error, info};

use crate::CONFIG;
use crate::database::Database;
use crate::database::ServerUpdateOperation;

pub struct DiscoveryScanner {
	pub database: Database,
}

impl DiscoveryScanner {
	/// Runs discovery with the method chosen by SCAN_METHOD ("masscan" by default, or "tcp").
	pub async fn scan(&self) {
		match std::env::var("SCAN_METHOD").as_deref() {
			Ok("tcp") => self.scan_tcp().await,
			_ => self.scan_masscan().await,
		}
	}

	/// Plain TCP connect scan, for environments where masscan can't open raw sockets.
	async fn scan_tcp(&self) {
		let (start, end) = match parse_range(&CONFIG.masscan.config_file) {
			Some(range) => range,
			None => {
				error!("Couldn't read a `range` from {}", CONFIG.masscan.config_file);
				std::process::exit(1);
			}
		};

		let concurrency: usize = std::env::var("SCAN_CONCURRENCY")
			.ok()
			.and_then(|v| v.parse().ok())
			.unwrap_or(1000);
		let semaphore = Arc::new(Semaphore::new(concurrency));
		let ports = CONFIG.scanner.port_range_start..=CONFIG.scanner.port_range_end;

		loop {
			info!("Starting TCP scan of {}-{}", Ipv4Addr::from(start), Ipv4Addr::from(end));

			for ip in start..=end {
				let address = Ipv4Addr::from(ip);

				for port in ports.clone() {
					let permit = semaphore.clone().acquire_owned().await.unwrap();
					let database = self.database.clone();

					tokio::spawn(async move {
						// Only servers that accept a connection get pinged
						let open = tokio::time::timeout(
							Duration::from_secs(2),
							TcpStream::connect(SocketAddrV4::new(address, port)),
						)
						.await
						.map(|r| r.is_ok())
						.unwrap_or(false);

						if open {
							handle_server(database, address, port).await;
						}

						drop(permit);
					});
				}
			}

			// Wait for in-flight connections to finish
			let _ = semaphore.acquire_many(concurrency as u32).await;
			info!("TCP scan finished");

			if !CONFIG.scanner.repeat {
				break;
			}

			tokio::time::sleep(Duration::from_secs(CONFIG.scanner.scan_delay)).await;
		}
	}

	async fn scan_masscan(&self) {
		// Spawn masscan
		let mut command = Command::new("sudo")
			.args(["masscan", "-c", &CONFIG.masscan.config_file])
			.stdout(std::process::Stdio::piped())
			.spawn()
			.expect("error while executing masscan");

		// Verify stdout is valid
		let stdout = command
			.stdout
			.take()
			.expect("Failed to get stdout from masscan!");

		let mut reader = BufReader::new(stdout).lines();

		// Iterate over the lines of output from masscan
		while let Ok(Some(line)) = reader.next_line().await {
			let mut line = line.split_whitespace();

			// Port
			let port = match line
				.nth(3)
				// Split on port/tcp
				.and_then(|p| p.split('/').nth(0))
				// Parse as u16
				.and_then(|s| s.parse().ok())
			{
				Some(port) => port,
				None => continue,
			};

			// Address
			let address = match line.nth(1).and_then(|a| Ipv4Addr::from_str(a).ok()) {
				Some(address) => address,
				None => continue,
			};

			// Immediately ping any returned servers
			handle_server(self.database.clone(), address, port).await;
		}
	}
}

/// Pings a server and stores whatever it returns.
async fn handle_server(database: Database, address: Ipv4Addr, port: u16) {
	if let Ok(Ok(server)) = super::run(address, port).await {
		let insertion_operation = ServerUpdateOperation {
			server,
			address: IpNet::from(Ipv4Net::from(address)),
			port: port as i32,
			timestamp: Utc::now().naive_local(),
			database,
		};

		// Update server
		if let Err(e) = insertion_operation.update_or_insert_server().await {
			debug!("Error while updating server ({address}:{port}) in database: {e}")
		}

		// Update players
		if let Err(e) = insertion_operation.update_or_insert_players().await {
			debug!("Error while updating players ({address}:{port}) in database: {e}")
		}

		// Update mods
		if let Err(e) = insertion_operation.update_or_insert_mods().await {
			debug!("Error while updating mods ({address}:{port}) in database: {e}")
		}
	}
}

/// Reads the `range` line of a masscan config. Accepts `a.b.c.d-e.f.g.h`, `a.b.c.d/n` or a single address.
fn parse_range(path: &str) -> Option<(u32, u32)> {
	let contents = std::fs::read_to_string(path).ok()?;
	let value = contents.lines().find_map(|line| {
		let (key, value) = line.split_once('=')?;
		(key.trim() == "range").then(|| value.trim().to_string())
	})?;

	if let Some((a, b)) = value.split_once('-') {
		return Some((
			u32::from(Ipv4Addr::from_str(a.trim()).ok()?),
			u32::from(Ipv4Addr::from_str(b.trim()).ok()?),
		));
	}

	if let Some((addr, bits)) = value.split_once('/') {
		let bits: u32 = bits.trim().parse().ok().filter(|b| *b <= 32)?;
		let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
		let base = u32::from(Ipv4Addr::from_str(addr.trim()).ok()?) & mask;
		return Some((base, base | !mask));
	}

	let ip = u32::from(Ipv4Addr::from_str(value.trim()).ok()?);
	Some((ip, ip))
}
