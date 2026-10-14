import pytest
from screentime.models import extract_domain


@pytest.mark.parametrize(
    "url, expected",
    [
        # Happy paths: full URLs
        ("https://example.com/path", "example.com"),
        ("http://example.com/path", "example.com"),
        ("https://www.example.com/path", "example.com"),
        ("http://www.example.com", "example.com"),

        # Missing protocols
        ("example.com/path", "example.com/path"),
        ("www.example.com/path", "example.com/path"),
        ("example.com", "example.com"),
        ("www.example.com", "example.com"),

        # Subdomains
        ("https://sub.example.com/path", "sub.example.com"),
        ("sub.example.com", "sub.example.com"),

        # Edge cases
        ("", None),
        (None, None),
        ("http://localhost:8080", "localhost"),
        ("localhost", "localhost"),
        ("127.0.0.1", "127.0.0.1"),
    ]
)
def test_extract_domain(url, expected):
    assert extract_domain(url) == expected
