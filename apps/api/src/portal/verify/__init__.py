"""Email verification (2FA-by-code) for links with verify_email on.

- email_check: cheap syntax + MX/A plausibility pre-filter.
- service: issue and check one-time codes (backed by the email_verifications table).
- trust: device-bound "this email is verified" cookie (30-day sliding window).
"""

from portal.verify.email_check import is_plausible_email
from portal.verify.gate import enforce_verification
from portal.verify.service import (
    InvalidEmail,
    ResendTooSoon,
    VerificationError,
    VerificationUnavailable,
    request_code,
    verify_code,
)
from portal.verify.trust import is_trusted, issue_trust

__all__ = [
    "InvalidEmail",
    "ResendTooSoon",
    "VerificationError",
    "VerificationUnavailable",
    "enforce_verification",
    "is_plausible_email",
    "is_trusted",
    "issue_trust",
    "request_code",
    "verify_code",
]
