//! The two ways a session can log in: the `dev-features` shortcut, or the real
//! SAML flow against the online TVS mock, which asks for nothing but a BSN.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use rand::RngExt;
use url::Url;

use crate::client::{Client, GetOutcome};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Login {
    /// `GET /dev/login`. Needs a server built with the `dev-features` feature.
    Dev,
    /// The SAML flow through the TVS mock the server is configured with, e.g.
    /// https://tvs-mock.eks-test.nl, logging in with a random BSN.
    TvsMock,
}

/// Logs in and lands on `/select-election`, leaving its CSRF token in
/// [`Client::csrf`].
pub async fn log_in(client: &mut Client, login: Login) -> Result<()> {
    match login {
        Login::Dev => dev_login(client).await,
        Login::TvsMock => tvs_mock_login(client, &Bsn::random()).await,
    }
}

/// `select_election=true` keeps the election picker in the flow instead of
/// dropping us straight into EK27, so `--election` and `--load-fixtures` still
/// mean something.
async fn dev_login(client: &mut Client) -> Result<()> {
    let next = match client
        .get("dev-login", "/dev/login?select_election=true")
        .await?
    {
        GetOutcome::Redirect(loc) => loc,
        GetOutcome::Page(_) => bail!(
            "/dev/login did not redirect (is the server built with the `dev-features` feature?)"
        ),
    };
    client
        .follow("select-election:get", next)
        .await
        .context("GET /select-election")?;
    Ok(())
}

/// The browser's side of the SAML flow. The two auto-submitted forms (the
/// AuthnRequest to the mock, the mock's redirect to the ACS) go out without
/// think time; clicking "Inloggen" and submitting the BSN are user actions.
async fn tvs_mock_login(client: &mut Client, bsn: &Bsn) -> Result<()> {
    client.get("login:get", "/login").await?;
    let authn_page = client
        .post("login:post", "/login", &[] as &[(&str, &str)])
        .await?
        .expect_page("login")?;
    let authn = HiddenForm::parse(&authn_page, &client.url("/login")?)
        .context("POST /login did not render the SAML auto-submit form")?;

    let bsn_page = client
        .autosubmit(
            "tvs-mock:authn-request",
            authn.action.as_str(),
            &authn.fields,
        )
        .await?
        .expect_page("tvs-mock authn request")?;
    let mut bsn_form = HiddenForm::parse(&bsn_page, &authn.action)
        .with_context(|| format!("{} did not render a BSN form", authn.action))?;
    bsn_form.fields.push(("bsn".into(), bsn.0.clone()));

    let acs = client
        .post("tvs-mock:bsn", bsn_form.action.as_str(), &bsn_form.fields)
        .await?
        .expect_redirect("tvs-mock bsn")?;
    let next = match client.fetch("saml-acs", &acs).await? {
        GetOutcome::Redirect(loc) => loc,
        GetOutcome::Page(_) => bail!("the ACS did not redirect (authentication failed?)"),
    };
    client
        .follow("select-election:get", next)
        .await
        .context("GET /select-election")?;
    Ok(())
}

/// A random BSN that passes the 11-proof. The mock's own "Nieuwe test-BSN"
/// button only draws from the 90k valid numbers starting with `999`, which
/// repeated runs would soon collide on; a colliding login lands on a stream an
/// earlier session already filled, where every person create fails the
/// uniqueness check.
pub struct Bsn(String);

impl Bsn {
    pub fn random() -> Self {
        let mut rng = rand::rng();
        loop {
            let prefix: u32 = rng.random_range(10_000_000..100_000_000);
            let digits = prefix.to_string();
            let sum: u32 = digits
                .bytes()
                .zip((2..=9).rev())
                .map(|(d, weight)| u32::from(d - b'0') * weight)
                .sum();
            let check = sum % 11;
            if check <= 9 && sum > check {
                return Self(format!("{digits}{check}"));
            }
        }
    }
}

/// The first `<form>` of a page: its action resolved against the page's URL,
/// and its hidden inputs.
struct HiddenForm {
    action: Url,
    fields: Vec<(String, String)>,
}

impl HiddenForm {
    fn parse(body: &str, page_url: &Url) -> Result<Self> {
        let (_, rest) = body.split_once("<form").context("no <form>")?;
        let (form, _) = rest.split_once("</form>").context("no </form>")?;
        let (form_tag, inputs) = form.split_once('>').context("unterminated <form>")?;
        let action = attr(form_tag, "action").context("form has no action")?;
        let action = page_url
            .join(action)
            .with_context(|| format!("join form action {action}"))?;
        let fields = inputs
            .split("<input")
            .skip(1)
            .filter_map(|input| input.split_once('>').map(|(tag, _)| tag))
            .filter(|tag| attr(tag, "type") == Some("hidden"))
            .filter_map(|tag| Some((attr(tag, "name")?.into(), attr(tag, "value")?.into())))
            .collect();
        Ok(Self { action, fields })
    }
}

/// Neither form carries HTML entities in its values (base64, ids and URLs), so
/// they are taken verbatim.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let (_, rest) = tag.split_once(&format!(" {name}=\""))?;
    rest.split_once('"').map(|(value, _)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passes_eleven_proof(bsn: &str) -> bool {
        let sum: i32 = bsn
            .bytes()
            .zip([9, 8, 7, 6, 5, 4, 3, 2, -1])
            .map(|(d, weight)| i32::from(d - b'0') * weight)
            .sum();
        sum > 0 && sum % 11 == 0
    }

    #[test]
    fn random_bsn_passes_eleven_proof() {
        for _ in 0..1000 {
            let bsn = Bsn::random();
            assert_eq!(bsn.0.len(), 9, "{}", bsn.0);
            assert!(passes_eleven_proof(&bsn.0), "{}", bsn.0);
        }
    }

    #[test]
    fn parses_the_mock_bsn_form() {
        let body = r#"<form method="POST" action="/kvs/rd/request_authentication" id="loginForm">
            <input type="hidden" name="AuthnRequestId" value="_abc" />
            <input type="hidden" name="RelayState" value="" />
            <input type="hidden" name="AcsUrl" value="https://dv.example/saml/sp/acs" />
            <input type="text" id="bsn" name="bsn" maxlength="9" value="999988888" />
        </form>"#;
        let page = Url::parse("https://mock.example/kvs/rd/request_authentication").unwrap();
        let form = HiddenForm::parse(body, &page).unwrap();
        assert_eq!(
            form.action.as_str(),
            "https://mock.example/kvs/rd/request_authentication"
        );
        assert_eq!(
            form.fields,
            [
                ("AuthnRequestId".into(), "_abc".into()),
                ("RelayState".into(), String::new()),
                ("AcsUrl".into(), "https://dv.example/saml/sp/acs".into()),
            ]
        );
    }
}
