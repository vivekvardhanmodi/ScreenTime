from dataclasses import dataclass
from typing import Optional
from urllib.parse import urlparse

def extract_domain(url: str) -> Optional[str]:
    """Extract the base domain from a URL."""
    if not url:
        return None
    try:
        parsed = urlparse(url)
        domain = parsed.hostname or parsed.path
        if domain and domain.startswith("www."):
            domain = domain[4:]
        return domain
    except Exception:
        return url

@dataclass
class Session:
    """A single activity session."""
    id: int
    start_time: float
    end_time: float
    app_class: str
    app_title: Optional[str]
    website_url: Optional[str]

    @property
    def duration(self) -> float:
        """Duration in seconds."""
        return self.end_time - self.start_time

    @property
    def display_name(self) -> str:
        """Human-readable name: website domain if available, else app class."""
        if self.website_url:
            domain = extract_domain(self.website_url)
            return domain or self.app_class
        return self.app_class

@dataclass
class AppUsage:
    """Aggregated usage for an app or website (or a group)."""
    name: str
    identifier_type: str  # 'app', 'website', or 'group'
    total_seconds: float
    category: Optional[str] = None
    children: Optional[list['AppUsage']] = None  # Individual items if this is a group

@dataclass
class Category:
    """A user-defined category."""
    id: int
    name: str
