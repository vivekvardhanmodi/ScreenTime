// screentime_kwin_tracker.js
// Polyfill and event listener for KDE Plasma 5 & 6

var isPlasma6 = (typeof workspace.windowActivated !== "undefined");

function getActiveWindow() {
    return isPlasma6 ? workspace.activeWindow : workspace.activeClient;
}

var currentTrackedWindow = null;

function notifyChange(win) {
    if (!win) {
        callDBus(
            "org.screentime.Daemon",
            "/org/screentime/Tracker",
            "org.screentime.Tracker",
            "WindowChanged",
            "",
            "",
            function() {}
        );
        return;
    }

    // resourceClass corresponds to app_id on Wayland and WM_CLASS on XWayland.
    // Fall back to desktopFileName or resourceName if resourceClass is empty.
    var appClass = win.resourceClass || win.desktopFileName || win.resourceName || "";
    var title = win.caption || "";

    callDBus(
        "org.screentime.Daemon",
        "/org/screentime/Tracker",
        "org.screentime.Tracker",
        "WindowChanged",
        String(appClass),
        String(title),
        function() {}
    );
}

function onCaptionChanged() {
    var win = getActiveWindow();
    notifyChange(win);
}

function onWindowActivated(win) {
    // Unhook previous caption listener if needed
    if (currentTrackedWindow && typeof currentTrackedWindow.captionChanged !== "undefined") {
        try {
            currentTrackedWindow.captionChanged.disconnect(onCaptionChanged);
        } catch (e) {
            // Safe to ignore if already disconnected
        }
    }

    currentTrackedWindow = win;

    if (win) {
        if (typeof win.captionChanged !== "undefined") {
            try {
                win.captionChanged.connect(onCaptionChanged);
            } catch (e) {
                // Ignore connection errors on exotic surfaces
            }
        }
        notifyChange(win);
    } else {
        notifyChange(null);
    }
}

// Register window activation listener
if (isPlasma6) {
    workspace.windowActivated.connect(onWindowActivated);
} else {
    workspace.clientActivated.connect(onWindowActivated);
}

// Initial notification for the currently focused window on startup
var initial = getActiveWindow();
if (initial) {
    onWindowActivated(initial);
} else {
    notifyChange(null);
}
