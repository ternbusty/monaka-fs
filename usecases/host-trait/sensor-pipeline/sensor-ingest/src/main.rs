//! Sensor Data Ingest Module
//! Simulates receiving sensor data and appending to a log file

use std::fs::{self, OpenOptions};
use std::io::Write;

fn main() {
    println!("=== Sensor Ingest Module ===");

    // Create data directory
    let _ = fs::create_dir("/data");

    // Simulate sensor readings (temperature, humidity)
    let readings = [
        (1, 23.5, 45.2),
        (2, 24.1, 44.8),
        (3, 23.8, 46.1),
        (4, 25.2, 43.5),
        (5, 24.7, 45.0),
    ];

    // Append sensor data to log file
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/data/sensor.log")
        .expect("Failed to open log file");

    for (id, temp, humidity) in readings {
        let line = format!("{},{:.1},{:.1}\n", id, temp, humidity);
        file.write_all(line.as_bytes()).expect("Failed to write");
        println!(
            "  Recorded: id={}, temp={:.1}C, humidity={:.1}%",
            id, temp, humidity
        );
    }

    println!(
        "  Wrote {} sensor readings to /data/sensor.log",
        readings.len()
    );
}
