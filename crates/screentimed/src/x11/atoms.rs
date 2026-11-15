use x11rb::atom_manager;

atom_manager! {
    pub Atoms: AtomsCookie {
        _NET_ACTIVE_WINDOW,
        _NET_WM_NAME,
        _NET_SUPPORTING_WM_CHECK,
        UTF8_STRING,
        WM_CLASS,
        WM_NAME,
    }
}
