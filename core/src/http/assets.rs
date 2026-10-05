use axum::Router;
use http::HeaderValue;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct FrontendAssets {
    storefront: PathBuf,
    admin: PathBuf,
    pub(crate) csp: HeaderValue,
}

impl FrontendAssets {
    pub fn load(storefront: &Path, admin: &Path) -> Result<Self, std::io::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Hashes {
            scripts: Vec<String>,
            styles: Vec<String>,
        }
        let hashes: Hashes =
            serde_json::from_slice(&std::fs::read(storefront.join("csp-hashes.json"))?)?;
        for hash in hashes.scripts.iter().chain(&hashes.styles) {
            if !hash.starts_with("sha256-")
                || hash.len() != 51
                || !hash[7..]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
            {
                return Err(std::io::Error::other("invalid frontend CSP hash"));
            }
        }
        std::fs::metadata(storefront.join("index.html"))?;
        std::fs::metadata(admin.join("index.html"))?;
        let scripts = hashes
            .scripts
            .iter()
            .map(|hash| format!("'{hash}'"))
            .collect::<Vec<_>>()
            .join(" ");
        let styles = hashes
            .styles
            .iter()
            .map(|hash| format!("'{hash}'"))
            .collect::<Vec<_>>()
            .join(" ");
        let csp = format!(
            "default-src 'self'; script-src 'self' {scripts}; style-src 'self' {styles}; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'"
        );
        Ok(Self {
            storefront: storefront.to_owned(),
            admin: admin.to_owned(),
            csp: HeaderValue::from_str(&csp).map_err(std::io::Error::other)?,
        })
    }

    pub(crate) fn attach(&self, router: Router) -> Router {
        router
            .route_service("/", ServeFile::new(self.storefront.join("index.html")))
            .nest_service("/_astro", ServeDir::new(self.storefront.join("_astro")))
            .nest_service("/admin/assets", ServeDir::new(self.admin.join("assets")))
            .route_service("/admin", ServeFile::new(self.admin.join("index.html")))
            .route_service("/admin/", ServeFile::new(self.admin.join("index.html")))
            .route_service(
                "/admin/{*path}",
                ServeFile::new(self.admin.join("index.html")),
            )
    }
}
