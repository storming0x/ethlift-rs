fn main() {
    if let Err(e) = ethlift::get_args().and_then(ethlift::run) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
