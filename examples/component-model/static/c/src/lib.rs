// Entry point for the C component
// This calls the C main function

extern "C" {
    fn main() -> i32;
}

#[no_mangle]
pub extern "C" fn _start() {
    unsafe {
        let exit_code = main();
        if exit_code != 0 {
            std::process::exit(exit_code);
        }
    }
}
