"""Cheap plausibility checks for an email address before a code is ever emailed.

Syntax catches `ze@`; an MX/A DNS lookup catches `ze@ze.ze` (no mail servers). Neither proves the
mailbox exists — the emailed code is the real proof — so DNS errors fail *open* (don't block a real
client on a transient resolver hiccup).
"""

from __future__ import annotations

import asyncio
import re

import dns.exception
import dns.resolver

_EMAIL_RE = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")
_DNS_TIMEOUT = 5.0


def valid_syntax(email: str) -> bool:
    return bool(_EMAIL_RE.match(email.strip()))


def _domain_receives_mail(domain: str) -> bool:
    resolver = dns.resolver.Resolver()
    resolver.timeout = _DNS_TIMEOUT
    resolver.lifetime = _DNS_TIMEOUT
    # MX is the definitive signal; fall back to A/AAAA (a domain can receive mail on its A record).
    for record in ("MX", "A", "AAAA"):
        try:
            answers = resolver.resolve(domain, record)
            if len(answers) > 0:
                return True
        except dns.resolver.NoAnswer:
            continue
        except dns.resolver.NXDOMAIN:
            return False  # domain doesn't exist at all
        except dns.exception.DNSException:
            return True  # transient/resolver error — fail open; the code send is the real gate
    return False


async def is_plausible_email(email: str, *, check_mx: bool) -> bool:
    email = email.strip()
    if not valid_syntax(email):
        return False
    if not check_mx:
        return True
    domain = email.rsplit("@", 1)[1]
    return await asyncio.to_thread(_domain_receives_mail, domain)
