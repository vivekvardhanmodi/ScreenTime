"""Shared utility functions for the ScreenTime TUI."""


def format_duration(seconds: float) -> str:
    """Format a duration in seconds to a human-readable string."""
    if seconds < 60:
        return f"{int(seconds)}s"
    elif seconds < 3600:
        mins = int(seconds // 60)
        secs = int(seconds % 60)
        return f"{mins}m {secs}s" if secs else f"{mins}m"
    else:
        hours = int(seconds // 3600)
        mins = int((seconds % 3600) // 60)
        return f"{hours}h {mins}m" if mins else f"{hours}h"


def make_bar(fraction: float, width: int = 40) -> str:
    """Create a text-based progress bar."""
    filled = int(fraction * width)
    filled = max(0, min(filled, width))
    return "█" * filled + "░" * (width - filled)
