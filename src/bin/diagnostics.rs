use battery_status::native_devices;

fn main() {
    println!("Battery Status — native HID diagnostics");
    println!("Battery queries only; no device settings are changed.\n");
    for device in native_devices::diagnostic_report() {
        println!("{device:#?}");
    }
    println!("\nReadings:");
    for reading in native_devices::poll_devices() {
        println!("{reading:#?}");
    }
}
