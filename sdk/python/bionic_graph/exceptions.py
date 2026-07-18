class BionicGraphError(Exception):
    """Base exception for all Bionic-Graph SDK errors."""


class ApiError(BionicGraphError):
    """An API call returned an error response."""

    def __init__(self, status_code: int, message: str, body: str = ""):
        self.status_code = status_code
        self.body = body
        super().__init__(f"[{status_code}] {message}")


class NotFoundError(ApiError):
    """Resource not found (HTTP 404)."""

    def __init__(self, message: str = "Not found", body: str = ""):
        super().__init__(404, message, body)


class ConnectionError(BionicGraphError):
    """Failed to connect to the Bionic-Graph server."""
