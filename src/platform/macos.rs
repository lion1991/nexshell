pub fn install_warp_ime_shims() {
    unsafe {
        nexshell_install_warp_ime_shims();
    }
}

unsafe extern "C" {
    fn nexshell_install_warp_ime_shims();
}
