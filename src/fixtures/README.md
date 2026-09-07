# Example persons

`persons.csv` is taken from the BRP mock the application is checked against,
[personen-mock](https://github.com/BRP-API/Haal-Centraal-BRP-bevragen/tree/master/test-data/personen-mock)
(`ghcr.io/brp-api/personen-mock`, `docker compose up -d personen-mock`).

Four out of five rows match the mock exactly, so the BRP check reports nothing
for them. The remaining fifth carries a mistake, together covering every finding
the check can produce: values the BRP holds differently or not at all, values it
holds in a shape this application cannot read, candidates it records as
deceased, not Dutch or excluded from the right to vote, residences without a
`woonplaats`, and burgerservicenummers that are missing, unknown or confirmed
absent. Three rows exercise the fallback search on personal details: a
typo'd number and a missing one are resolved to the right person anyway, and
the confirmed-absent one belongs to a `Precise` sibling, ten of whom share a
surname and date of birth, so no combination of details can tell them apart. Only `BsnNotUnique` is absent, which the mock
cannot serve because it keys its records on the burgerservicenummer.

`brp_agrees_with_four_out_of_five_fixture_candidates` in `persons.rs` checks
this against the running mock. The rows with a mistake are spread over the first
fifty-five, so they end up on the fixture candidate list.

## Columns

`geslachtsnaam` holds the prefix as it is written on the candidate list ("de
Bruin"); `split_last_name_prefix` splits it off again. The address columns are
the person's correspondence address and are not checked against the BRP; the
`woonplaats` is.

## Regenerating from the mock

```sh
fields=$(jq -r '.[] | [
    .burgerservicenummer,
    .geslacht.code,
    .naam.voornamen,
    ((.naam.voorvoegsel // "") + " " + .naam.geslachtsnaam | ltrimstr(" ")),
    .geboorte.datum,
    .verblijfplaats.straat,
    .verblijfplaats.huisnummer,
    .verblijfplaats.postcode,
    .verblijfplaats.woonplaats
] | @csv' test-data.json)

echo "burgerservicenummer,geslacht,voornamen,geslachtsnaam,geboortedatum,straat,huisnummer,postcode,woonplaats" > output.csv
echo "$fields" >> output.csv
```
