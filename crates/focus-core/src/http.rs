//! The HTTP client a source gets from the host. It never follows redirects,
//! so a source can tell a 200 from a 3xx.

use std::time::Duration;

use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// The `Location` header of a redirect.
    pub location: Option<String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status)
    }
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    /// GET `url` without following redirects. `Err` is a transport failure
    /// (DNS, TLS, timeout); any HTTP status is `Ok`.
    async fn get(&self, url: &str) -> Result<HttpResponse, String>;
}

/// `reqwest` with rustls, no redirects and a short timeout.
#[derive(Debug, Clone)]
pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("focus-engine/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(ReqwestHttp { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestHttp {
    async fn get(&self, url: &str) -> Result<HttpResponse, String> {
        let resp = self
            .client
            .get(url)
            // Skips the EU cookie-consent interstitial, which would otherwise
            // turn every Shorts check into a redirect.
            .header(reqwest::header::COOKIE, "SOCS=CAI; CONSENT=YES+")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
        Ok(HttpResponse {
            status,
            location,
            body,
        })
    }
}
