// @group Security : Outbound URL validation and DNS pinning for credential-bearing requests

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{redirect::Policy, Client, Url};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundPolicy {
    PublicHttps,
    LoopbackHttp,
}

pub fn validate_url(raw: &str, policy: OutboundPolicy) -> Result<Url> {
    if raw.len() > 2_048 {
        bail!("URL is too long");
    }
    let url = Url::parse(raw).context("URL is invalid")?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("URL credentials are not allowed");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL host is required"))?;
    let host_ip = host.trim_matches(['[', ']']).parse::<IpAddr>();

    match policy {
        OutboundPolicy::PublicHttps => {
            if url.scheme() != "https" {
                bail!("only HTTPS public endpoints are allowed");
            }
            if host.eq_ignore_ascii_case("localhost") {
                bail!("loopback and private endpoints are not allowed");
            }
            if let Ok(ip) = host_ip {
                if !is_public_ip(ip) {
                    bail!("loopback and private endpoints are not allowed");
                }
            }
        }
        OutboundPolicy::LoopbackHttp => {
            if !matches!(url.scheme(), "http" | "https") {
                bail!("local provider URL must use HTTP or HTTPS");
            }
            if !host.eq_ignore_ascii_case("localhost")
                && host_ip.ok().is_none_or(|ip| !ip.is_loopback())
            {
                bail!("local provider URL must use localhost or a loopback IP");
            }
        }
    }

    Ok(url)
}

/// Build a no-redirect client and pin the validated hostname to a public DNS result.
pub async fn client_for_url(url: &Url, policy: OutboundPolicy) -> Result<Client> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL host is required"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("URL port is required"))?;

    let mut builder = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15));

    if host.trim_matches(['[', ']']).parse::<IpAddr>().is_err() {
        const MAX_DNS_RESULTS: usize = 16;
        let lookup = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .context("endpoint DNS lookup timed out")?
        .context("endpoint DNS lookup failed")?;
        let resolved: Vec<SocketAddr> = lookup.take(MAX_DNS_RESULTS + 1).collect();
        if resolved.len() > MAX_DNS_RESULTS {
            bail!("endpoint DNS returned too many addresses");
        }
        let addresses_are_allowed = match policy {
            OutboundPolicy::PublicHttps => {
                resolved.iter().all(|address| is_public_ip(address.ip()))
            }
            OutboundPolicy::LoopbackHttp => {
                resolved.iter().all(|address| address.ip().is_loopback())
            }
        };
        if resolved.is_empty() || !addresses_are_allowed {
            bail!("endpoint DNS resolved outside its allowed network boundary");
        }
        builder = builder.resolve(host, resolved[0]);
    }

    builder.build().context("failed to build outbound client")
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, d] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && (b == 18 || b == 19))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
        || (a == 255 && b == 255 && c == 255 && d == 255))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_policy_rejects_private_and_credentialed_urls() {
        assert!(validate_url("http://example.com/hook", OutboundPolicy::PublicHttps).is_err());
        assert!(validate_url("https://127.0.0.1/hook", OutboundPolicy::PublicHttps).is_err());
        assert!(validate_url(
            "https://169.254.169.254/latest",
            OutboundPolicy::PublicHttps
        )
        .is_err());
        assert!(validate_url(
            "https://user:pass@example.com/hook",
            OutboundPolicy::PublicHttps
        )
        .is_err());
        assert!(validate_url("https://example.com/hook", OutboundPolicy::PublicHttps).is_ok());
    }

    #[test]
    fn local_policy_only_accepts_loopback() {
        assert!(validate_url("http://localhost:11434", OutboundPolicy::LoopbackHttp).is_ok());
        assert!(validate_url("http://[::1]:11434", OutboundPolicy::LoopbackHttp).is_ok());
        assert!(validate_url("http://192.168.1.2:11434", OutboundPolicy::LoopbackHttp).is_err());
        assert!(validate_url("https://example.com", OutboundPolicy::LoopbackHttp).is_err());
    }
}
