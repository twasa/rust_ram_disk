fn main() {
    #[cfg(feature = "winfsp-mount")]
    winfsp::build::winfsp_link_delayload();
}
