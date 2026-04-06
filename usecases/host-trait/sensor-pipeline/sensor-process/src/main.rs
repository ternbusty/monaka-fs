//! Sensor Data Process Module
//! Reads sensor log and performs statistical analysis

use std::fs;

fn main() {
    println!("=== Sensor Process Module ===");

    // Read sensor log
    let content = fs::read_to_string("/data/sensor.log").expect("Failed to read sensor log");

    let mut temps: Vec<f64> = Vec::new();
    let mut humidities: Vec<f64> = Vec::new();

    // Parse CSV data
    for line in content.lines() {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() == 3 {
            if let (Ok(temp), Ok(humidity)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
                temps.push(temp);
                humidities.push(humidity);
            }
        }
    }

    println!("  Read {} sensor readings", temps.len());

    if !temps.is_empty() {
        // Calculate statistics
        let temp_avg = temps.iter().sum::<f64>() / temps.len() as f64;
        let temp_min = temps.iter().cloned().fold(f64::INFINITY, f64::min);
        let temp_max = temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let humidity_avg = humidities.iter().sum::<f64>() / humidities.len() as f64;

        println!();
        println!("  Temperature Statistics:");
        println!("    Average: {:.2}C", temp_avg);
        println!("    Min:     {:.2}C", temp_min);
        println!("    Max:     {:.2}C", temp_max);
        println!();
        println!("  Humidity Statistics:");
        println!("    Average: {:.2}%", humidity_avg);
        println!();
        println!("  Analysis complete!");
    }
}
