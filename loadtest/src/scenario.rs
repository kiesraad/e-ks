//! The realistic per-user flow, expressed as a flat top-to-bottom script.
//!
//! The CSRF token is stable for the lifetime of a session — we sniff it once
//! from the first rendered page and reuse it for every POST. GETs interleaved
//! between POSTs are there for realism (a real user does navigate to a form
//! before submitting it), not to refresh CSRF.
//!
//! When the actions a user does change, the only file that needs to change is
//! this one.

use anyhow::{Context, Result, bail};
use rand::seq::SliceRandom;

use crate::client::{Client, GetOutcome};
use crate::data::{PersonRow, unique_last_name};

pub struct ScenarioConfig {
    pub persons_per_user: usize,
    pub election: &'static str,
    pub load_fixtures_via_form: bool,
    pub reorders: usize,
    pub edits: usize,
    pub locale: &'static str,
}

pub async fn run_session(
    client: &mut Client,
    persons: &[PersonRow],
    suffix: &str,
    config: &ScenarioConfig,
) -> Result<()> {
    // 1. Login. The real flow is DigiD/TVS SAML, which a load test can't drive,
    //    so we use the `dev-features` shortcut. `select_election=true` keeps the
    //    election picker in the flow instead of dropping us straight into EK27,
    //    so `--election` and `--load-fixtures` still mean something.
    let next = match client
        .get("dev-login", "/dev/login?select_election=true")
        .await?
    {
        GetOutcome::Redirect(loc) => loc,
        GetOutcome::Page(_) => bail!("/dev/login did not redirect (is the server built with the `dev-features` feature?)"),
    };

    // 2. Follow to /select-election. The first rendered page sets the CSRF
    //    token, which is stable for the rest of the session.
    client
        .follow("select-election:get", next)
        .await
        .context("GET /select-election")?;
    let csrf = client.csrf().to_string();

    // 3. Submit the election choice.
    let mut form: Vec<(&str, &str)> = vec![("csrf_token", &csrf), ("election", config.election)];
    if config.load_fixtures_via_form {
        form.push(("load_fixtures", "true"));
    }
    let after_select = client
        .post("select-election:post", "/select-election", &form)
        .await?
        .expect_redirect("select-election")?;
    client.follow("index", after_select).await?;

    // 4. Browse around (realism: a user doesn't only POST).
    client.get("persons:list", "/persons").await?;
    client.get("list-designation:get", "/political-group").await?;
    client
        .get("political-group:get", "/political-group/information")
        .await?;
    client.get("audit-log:get", "/audit-log").await?;
    client.get("candidate-lists:list", "/candidate-lists").await?;

    // 5. Create each person + their address. The user navigates to the form
    //    page each time, mirroring real behaviour. Per-candidate validation
    //    failures (bad date, BSN that fails the 11-proof, candidate too young)
    //    are real and expected — log them and move on instead of aborting the
    //    session.
    // Persons that pass form validation but are missing the fields the model
    // generator requires (date of birth, place of residence) get tracked
    // separately so we can drop them from the candidate list before the
    // download step — see step 11.
    let mut complete: Vec<(String, &PersonRow)> = Vec::new();
    let mut incomplete_ids: Vec<String> = Vec::new();
    for (i, row) in persons.iter().take(config.persons_per_user).enumerate() {
        client.get("persons:list", "/persons").await?;
        let row_suffix = format!("{suffix}-{i}");
        match create_person(client, &csrf, row, &row_suffix).await {
            Ok((id, true)) => complete.push((id, row)),
            Ok((id, false)) => incomplete_ids.push(id),
            Err(err) => {
                eprintln!("skip {} {}: {err}", row.geslachtsnaam, row.first_name());
            }
        }
    }

    // 6. Correct a few of them afterwards. The update form posts the *whole*
    //    personal-data form (it is `#[serde(default)]`, so a partial POST would
    //    silently blank out date of birth, BSN and place of residence and drop
    //    the candidate out of the models), so we resend the row with an
    //    amended last name.
    for (i, (person_id, row)) in complete.iter().take(config.edits).enumerate() {
        let last_name = format!("{} aangepast", unique_last_name(&row.geslachtsnaam, &format!("{suffix}-{i}")));
        if let Err(err) = edit_person(client, &csrf, person_id, row, &last_name).await {
            eprintln!("edit {person_id}: {err}");
        }
    }

    // 7. Pick the list designation, then fill in the group's display name. The
    //    designation drives which H3 model is generated, so it comes first.
    update_list_designation(client, &csrf, "standalone").await?;
    update_political_group(client, &csrf, suffix).await?;

    // 8. The rest of the general-information flow that the fixture loader
    //    replicates: one name authorisation (formerly "authorised agent", and
    //    the holder of the legal name), the (singleton) list submitter, and two
    //    substitute submitters.
    create_name_authorisation(client, &csrf, suffix).await?;
    update_list_submitter(client, &csrf).await?;
    create_substitute_submitter(
        client, &csrf, "Smit", Some("van"), "G.H.", "Spui", "18", None, "2511 DD", "Den Haag",
    )
    .await?;
    create_substitute_submitter(
        client, &csrf, "Jong", None, "I.J.", "Oudegracht", "21", Some("C"), "3511 AA", "Utrecht",
    )
    .await?;

    // 9. Create a candidate list. EK27 has 16 electoral districts, so the GET
    //    renders the picker and the POST creates the list; its 303 carries the
    //    new `list_id`.
    let candidate_list_id = create_candidate_list(client, &csrf).await?;

    // 10. Add every person we created to the candidate list.
    add_all_persons_to_list(client, &csrf, &candidate_list_id).await?;

    // 11. The PDF/eml generators reject the whole list if any candidate is
    //     missing a date of birth or place of residence. The person-level
    //     form validator allows those fields to be empty, so they slip onto
    //     the list. Drop them now so reorder + download can succeed.
    for id in &incomplete_ids {
        delete_candidate(client, &csrf, &candidate_list_id, id).await?;
    }

    // 12. Shuffle the (now-clean) order a few times to exercise the reorder
    //     endpoint.
    let mut complete_ids: Vec<String> = complete.into_iter().map(|(id, _)| id).collect();
    for _ in 0..config.reorders {
        reorder_list(client, &candidate_list_id, &mut complete_ids).await?;
    }

    // 13. Download the documents. There is a single all-in-one zip per locale
    //     now, covering every candidate list at once; it is timed as a full
    //     transfer (the Typst service has to render every model), so it's
    //     usually the slowest leg of the run by a wide margin.
    download_documents(client, &csrf, config.locale).await?;

    // 14. Final survey of the data we created.
    client.get("persons:list", "/persons").await?;
    client.get("candidate-lists:list", "/candidate-lists").await?;

    Ok(())
}

/// The personal-data form fields for a fixture row. The update form reuses this
/// (with an amended last name) because it is `#[serde(default)]` server-side:
/// any field left out of the POST is cleared, not kept.
fn personal_data_form<'a>(
    csrf: &'a str,
    row: &'a PersonRow,
    last_name: &'a str,
    initials: &'a str,
    date_of_birth: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("csrf_token", csrf),
        ("first_name", row.first_name()),
        ("last_name", last_name),
        ("last_name_prefix", ""),
        ("initials", initials),
        ("gender", row.gender()),
        ("date_of_birth", date_of_birth),
        ("bsn", &row.burgerservicenummer),
        ("place_of_residence", &row.woonplaats),
        ("country", "NL"),
    ]
}

/// Creates a person + address. Returns `(person_id, complete_for_download)`,
/// where the second flag is true iff this candidate has the fields the model
/// generator requires (date of birth + place of residence). The form-level
/// validators allow both to be empty, so a person with `complete = false` is
/// still successfully created.
async fn create_person(
    client: &mut Client,
    csrf: &str,
    row: &PersonRow,
    suffix: &str,
) -> Result<(String, bool)> {
    client.get("person-create:get", "/persons/create").await?;

    let last_name = unique_last_name(&row.geslachtsnaam, suffix);
    let initials = row.initials();
    let dob = row.date_of_birth();
    let complete = !dob.is_empty() && !row.woonplaats.is_empty();
    let form = personal_data_form(csrf, row, &last_name, &initials, &dob);
    let address_path = client
        .post("person-create:post", "/persons/create", &form)
        .await?
        .expect_redirect("create person")?;
    let person_id = parse_person_id(&address_path).ok_or_else(|| {
        anyhow::anyhow!("could not extract person_id from redirect: {address_path}")
    })?;

    // The address redirect carries `?created=true`. Land on it (realistic),
    // then POST to the canonical path.
    client.get("person-address:get", &address_path).await?;
    let address_form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("street_name", &row.straat),
        ("house_number", &row.huisnummer),
        ("house_number_addition", ""),
        ("postal_code", &row.postcode),
        ("locality", &row.woonplaats),
    ];
    let after_address = client
        .post(
            "person-address:post",
            &strip_query(&address_path),
            &address_form,
        )
        .await?
        .expect_redirect("update address")?;
    client.follow("persons:list", after_address).await?;
    Ok((person_id, complete))
}

async fn edit_person(
    client: &mut Client,
    csrf: &str,
    person_id: &str,
    row: &PersonRow,
    new_last_name: &str,
) -> Result<()> {
    let path = format!("/persons/{person_id}/update");
    client.get("person-edit:get", &path).await?;

    let initials = row.initials();
    let dob = row.date_of_birth();
    let form = personal_data_form(csrf, row, new_last_name, &initials, &dob);
    // A Dutch-resident person lands back on their address page after an update
    // (the "next step" in the person flow), so follow wherever we're sent.
    let next = client
        .post("person-edit:post", &path, &form)
        .await?
        .expect_redirect("edit person")?;

    client.follow("person-address:get", next).await?;
    Ok(())
}

async fn delete_candidate(
    client: &mut Client,
    csrf: &str,
    list_id: &str,
    person_id: &str,
) -> Result<()> {
    let path = format!("/candidate-lists/{list_id}/delete/{person_id}");
    let form: Vec<(&str, &str)> = vec![("csrf_token", csrf)];
    let next = client
        .post("candidate:delete", &path, &form)
        .await?
        .expect_redirect("delete candidate")?;
    client.follow("candidate-list:view", next).await?;
    Ok(())
}

async fn add_all_persons_to_list(client: &mut Client, csrf: &str, list_id: &str) -> Result<()> {
    let path = format!("/candidate-lists/{list_id}/add");
    client.get("candidate-list:add:get", &path).await?;
    let form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("action", "add-all"),
        ("added_position", ""),
    ];
    // The add endpoint re-renders the same HTML on both success and validation
    // failure (the page is a modal that updates in place), so a 200 here is
    // the happy path — we don't get a redirect.
    client.post("candidate-list:add:post", &path, &form).await?;
    client
        .get("candidate-list:view", &format!("/candidate-lists/{list_id}"))
        .await?;
    Ok(())
}

async fn reorder_list(
    client: &mut Client,
    list_id: &str,
    person_ids: &mut Vec<String>,
) -> Result<()> {
    person_ids.shuffle(&mut rand::rng());
    let payload = serde_json::json!({ "person_ids": person_ids });
    let path = format!("/candidate-lists/{list_id}/reorder");
    client
        .post_json("candidate-list:reorder", &path, &payload)
        .await?
        .expect_no_content("reorder")?;
    // Follow up by viewing the list — that's what a real user does after
    // dragging the rows around.
    client
        .get("candidate-list:view", &format!("/candidate-lists/{list_id}"))
        .await?;
    Ok(())
}

async fn create_candidate_list(client: &mut Client, csrf: &str) -> Result<String> {
    // EK27 has 16 electoral districts — pick them all. (For single-district
    // elections like PS27/WS27, the GET handler auto-creates the list and
    // we'd skip the POST; we don't bother handling that here since the rest
    // of the scenario is EK27-flavoured anyway.)
    let path = "/candidate-lists/create";
    client.get("candidate-list:create:get", path).await?;
    let mut form: Vec<(&str, &str)> = vec![("csrf_token", csrf)];
    for district in EK27_DISTRICTS {
        form.push(("electoral_districts", district));
    }
    let target = client
        .post("candidate-list:create:post", path, &form)
        .await?
        .expect_redirect("create candidate list")?;
    let list_id = parse_list_id(&target)
        .ok_or_else(|| anyhow::anyhow!("could not extract list_id from redirect: {target}"))?;
    client.follow("candidate-list:view", target).await?;
    Ok(list_id)
}

/// The `ElectoralDistrict` variant names EK27 uses. These are the *form* values
/// (`serde_name()`), which for these 16 happen to equal their codes.
const EK27_DISTRICTS: &[&str] = &[
    "GR", "FR", "DR", "OV", "FL", "GE", "UT", "NH", "ZH", "ZE", "NB", "LI", "BO", "SE", "SA", "KN",
];

/// One download per session: `/generate/{locale}/documents.zip` bundles every
/// model (H1, H3, H4, H9) plus the EML 2.10 export for every candidate list,
/// replacing the per-model endpoints.
async fn download_documents(client: &mut Client, csrf: &str, locale: &str) -> Result<()> {
    client.get("finalise:get", "/finalise").await?;
    let path = format!("/generate/{locale}/documents.zip");
    if let Err(err) = client.download("download:documents", &path).await {
        eprintln!("download documents: {err}");
        return Ok(());
    }

    // After a download the app shows a "don't forget to check the documents"
    // warning on every page until the user dismisses it.
    let form: Vec<(&str, &str)> = vec![("csrf_token", csrf)];
    let next = client
        .post_with_referer(
            "hide-download-warning:post",
            "/hide-download-warning",
            &form,
            "/finalise",
        )
        .await?
        .expect_redirect("hide download warning")?;
    client.follow("finalise:get", next).await?;
    Ok(())
}

fn parse_list_id(redirect: &str) -> Option<String> {
    parse_id_segment(redirect, "/candidate-lists/")
}

fn parse_person_id(redirect: &str) -> Option<String> {
    parse_id_segment(redirect, "/persons/")
}

fn parse_id_segment(redirect: &str, prefix: &str) -> Option<String> {
    let id = redirect
        .strip_prefix(prefix)?
        .split('?')
        .next()?
        .split('/')
        .next()?;
    if id.is_empty() { None } else { Some(id.to_string()) }
}

async fn create_name_authorisation(client: &mut Client, csrf: &str, suffix: &str) -> Result<()> {
    let path = "/political-group/name-authorisation/create";
    client.get("name-authorisation:create:get", path).await?;
    let legal = format!("Vereniging Partij {suffix}");
    let form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("last_name", "Jansen"),
        ("last_name_prefix", "de"),
        ("initials", "A.B."),
        ("legal_name", &legal),
    ];
    let next = client
        .post("name-authorisation:create:post", path, &form)
        .await?
        .expect_redirect("create name authorisation")?;
    client.follow("name-authorisations:list", next).await?;
    Ok(())
}

async fn update_list_submitter(client: &mut Client, csrf: &str) -> Result<()> {
    let path = "/political-group/list-submitter/update";
    client.get("list-submitter:update:get", path).await?;
    let form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("last_name", "Bos"),
        ("last_name_prefix", ""),
        ("initials", "E.F."),
        // InternationalAddressForm fields (empty `country` -> Dutch address)
        ("country", ""),
        ("locality", "Rotterdam"),
        ("state_or_province", ""),
        ("postal_code", "3011 CC"),
        ("house_number", "5"),
        ("house_number_addition", "B"),
        ("street_name", "Coolsingel"),
    ];
    let next = client
        .post("list-submitter:update:post", path, &form)
        .await?
        .expect_redirect("update list submitter")?;
    client.follow("list-submitter:view", next).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_substitute_submitter(
    client: &mut Client,
    csrf: &str,
    last_name: &str,
    last_name_prefix: Option<&str>,
    initials: &str,
    street_name: &str,
    house_number: &str,
    house_number_addition: Option<&str>,
    postal_code: &str,
    locality: &str,
) -> Result<()> {
    let path = "/political-group/substitute-submitters/create";
    client.get("substitute-submitter:create:get", path).await?;
    let form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("last_name", last_name),
        ("last_name_prefix", last_name_prefix.unwrap_or("")),
        ("initials", initials),
        ("country", ""),
        ("locality", locality),
        ("state_or_province", ""),
        ("postal_code", postal_code),
        ("house_number", house_number),
        ("house_number_addition", house_number_addition.unwrap_or("")),
        ("street_name", street_name),
    ];
    let next = client
        .post("substitute-submitter:create:post", path, &form)
        .await?
        .expect_redirect("create substitute submitter")?;
    client.follow("list-submitter:view", next).await?;
    Ok(())
}

/// `standalone` / `blank` / `combined`. Which one is set decides whether an H3-1
/// or an H3-2 ends up in the zip, so it has to be set before downloading.
async fn update_list_designation(client: &mut Client, csrf: &str, designation: &str) -> Result<()> {
    client.get("list-designation:get", "/political-group").await?;
    let form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("list_designation_type", designation),
    ];
    let next = client
        .post("list-designation:post", "/political-group", &form)
        .await?
        .expect_redirect("list designation update")?;
    client.follow("political-group:get", next).await?;
    Ok(())
}

async fn update_political_group(client: &mut Client, csrf: &str, suffix: &str) -> Result<()> {
    let path = "/political-group/information";
    client.get("political-group:get", path).await?;
    let display = format!("Partij {suffix}");
    let form: Vec<(&str, &str)> = vec![
        ("csrf_token", csrf),
        ("display_name", &display),
        ("previous_election_results", ""),
    ];
    let next = client
        .post("political-group:post", path, &form)
        .await?
        .expect_redirect("political group update")?;
    client.follow("name-authorisations:list", next).await?;
    Ok(())
}

fn strip_query(path: &str) -> String {
    path.split_once('?')
        .map(|(p, _)| p.to_string())
        .unwrap_or_else(|| path.to_string())
}
