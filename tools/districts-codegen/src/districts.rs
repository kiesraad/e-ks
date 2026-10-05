use std::path::Path;

use eml_nl::{
    EMLVersion,
    documents::master_election_tree::{MasterElectionTree, MetRegion},
    io::EMLRead,
    utils::RegionCategory,
};

use crate::utils::{NBSB_REGION_NUMBER, num, to_ident};

/// Regions from MasterElectionTree, sorted by category and election type
#[derive(Default)]
pub(crate) struct Districts {
    /// EK/PS: PROVINCIE + KIESCOLLEGE
    pub(crate) provinces_and_colleges: Vec<MetRegion>,
    /// EK: PROVINCIAAL_STEMBUREAU; tuple is (region, parent EK number)
    pub(crate) province_polling_stations: Vec<(MetRegion, u16)>,
    /// TK: KIESKRING
    pub(crate) electoral_districts: Vec<MetRegion>,
    /// PS: PROVINCIAAL_KIESKRING; tuple is (region, parent PROVINCIE number)
    pub(crate) province_electoral_district: Vec<(MetRegion, u16)>,
    /// TK/PS: GEMEENTE; tuple is (region, parent PROVINCIAAL_KIESKRING number, grandparent PROVINCIE number)
    pub(crate) municipality: Vec<(MetRegion, u16, u16)>,
    /// WS: WATERSCHAP
    pub(crate) water_authority: Vec<MetRegion>,
    /// WS: WATERSCHAP_KIESKRING; tuple is (region, parent WATERSCHAP number)
    pub(crate) water_authority_electoral_district: Vec<(MetRegion, u16)>,
    /// WS: WATERSCHAP_GEMEENTE references; tuple is (WATERSCHAP_GEMEENTE region, base Gm ident, parent WATERSCHAP number, FrysianExportAllowed)
    /// When FrysianExportAllowed differs from the TK/PS gemeente, a separate Fr-suffixed variant is generated
    pub(crate) water_authority_municipality: Vec<(MetRegion, String, u16, bool)>,
}

impl Districts {
    pub(crate) fn parse_from_file(file: &Path) -> Self {
        let xml = std::fs::read_to_string(file).expect("Could not read MasterElectionTree.xml");
        let tree = MasterElectionTree::parse_eml_fragment(
            &xml,
            eml_nl::io::EMLParsingMode::Strict,
            EMLVersion::default(),
        )
        .expect("Failed to parse MasterElectionTree.xml");

        Districts::from(tree)
    }
}

/// One ElectoralDistrict variant with its precomputed names
pub(crate) struct RegionEntry<'a> {
    /// Rust enum variant name, e.g. "SbGroningen", "GmLeeuwarden"
    pub(crate) variant: String,
    /// Stable typed code for use in e.g. the EML API, e.g. "sb2", "gm358"
    pub(crate) code: String,
    pub(crate) region: &'a MetRegion,
}

impl Districts {
    /// All districts in enum declaration order, with precomputed variant name and code
    #[allow(clippy::too_many_lines)]
    pub(crate) fn all_entries<'a>(&'a self) -> Vec<RegionEntry<'a>> {
        let mut entries = Vec::new();
        for r in &self.provinces_and_colleges {
            let code_prefix = match r.key.category {
                RegionCategory::Province => "prov",
                RegionCategory::ElectoralCollege => "kc",
                _ => panic!("provinces_and_colleges should only contain provinces and colleges"),
            };
            entries.push(RegionEntry {
                variant: to_ident(&r.name),
                code: format!("{}{}", code_prefix, num(r)),
                region: r,
            });
        }
        for (r, _) in &self.province_polling_stations {
            entries.push(RegionEntry {
                variant: format!("Sb{}", to_ident(&r.name)),
                code: format!("sb{}", num(r)),
                region: r,
            });
        }
        for r in &self.electoral_districts {
            entries.push(RegionEntry {
                variant: format!("Tk{}", to_ident(&r.name)),
                code: format!("tk{}", num(r)),
                region: r,
            });
        }
        for (r, _) in &self.province_electoral_district {
            entries.push(RegionEntry {
                variant: format!("Ps{}", to_ident(&r.name)),
                code: format!("pk{}", num(r)),
                region: r,
            });
        }
        // The "Fr" suffix indicates a municipality has FrysianExportAllowed true.
        // Some municipalities appear both in Fryslan and in another water council,
        // so we use the suffix to remember when Frisian export is allowed.
        for (r, _, _) in &self.municipality {
            let suffix = if r.frysian_export_allowed { "Fr" } else { "" };
            entries.push(RegionEntry {
                variant: format!("Gm{}{}", to_ident(&r.name), suffix),
                code: format!("gm{}{}", num(r), suffix.to_lowercase()),
                region: r,
            });
        }
        for (r, ws_num) in &self.water_authority_electoral_district {
            entries.push(RegionEntry {
                variant: format!("Ws{}", to_ident(&r.name)),
                code: format!("ws{}", ws_num),
                region: r,
            });
        }
        // WS gemeente variants that are not already covered by PS
        let existing: std::collections::HashSet<String> =
            entries.iter().map(|e| e.variant.clone()).collect();
        let mut ws_extras: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (gm_region, gm_ident, _, frisian_flag) in &self.water_authority_municipality {
            let suffix = if *frisian_flag { "Fr" } else { "" };
            let variant = format!("{}{}", gm_ident, suffix);
            if !existing.contains(&variant) && ws_extras.insert(variant.clone()) {
                entries.push(RegionEntry {
                    variant,
                    code: format!("gm{}{}", num(gm_region), suffix.to_lowercase()),
                    region: gm_region,
                });
            }
        }
        entries
    }
}

impl From<MasterElectionTree> for Districts {
    #[allow(clippy::too_many_lines)]
    fn from(tree: MasterElectionTree) -> Self {
        let staat = &tree.root;
        let mut d = Districts::default();

        // Filter helper: iterate child regions of a given category
        let of = |cat: RegionCategory| move |r: &&MetRegion| r.key.category == cat;

        // PROVINCIE: EK provinces with their nested structure
        for prov in staat.subregions.iter().filter(of(RegionCategory::Province)) {
            let prov_num = num(prov);
            d.provinces_and_colleges.push(prov.clone());
            for sb in prov
                .subregions
                .iter()
                .filter(of(RegionCategory::ProvincePollingStation))
            {
                d.province_polling_stations.push((sb.clone(), prov_num));
                for ks in sb
                    .subregions
                    .iter()
                    .filter(of(RegionCategory::ElectoralDistrict))
                {
                    d.electoral_districts.push(ks.clone());
                    for pk in ks
                        .subregions
                        .iter()
                        .filter(of(RegionCategory::ProvinceElectoralDistrict))
                    {
                        let pk_num = num(pk);
                        d.province_electoral_district.push((pk.clone(), prov_num));
                        for gm in pk
                            .subregions
                            .iter()
                            .filter(of(RegionCategory::Municipality))
                            .filter(|r| num(r) != NBSB_REGION_NUMBER)
                        {
                            d.municipality.push((gm.clone(), pk_num, prov_num));
                        }
                    }
                }
            }
        }

        // KIESCOLLEGE: EK electoral colleges with their STEMBUREAU
        for kc in staat
            .subregions
            .iter()
            .filter(of(RegionCategory::ElectoralCollege))
        {
            let kc_num = num(kc);
            d.provinces_and_colleges.push(kc.clone());
            for sb in kc
                .subregions
                .iter()
                .filter(of(RegionCategory::ProvincePollingStation))
            {
                d.province_polling_stations.push((sb.clone(), kc_num));
            }
        }

        // KIESKRING directly under STAAT (Bonaire special case)
        for ks in staat
            .subregions
            .iter()
            .filter(of(RegionCategory::ElectoralDistrict))
        {
            d.electoral_districts.push(ks.clone());
        }

        // WATERSCHAP: WS water authorities with their municipalities
        // Build a CBS-code -> Gm ident lookup from already-collected PS municipalities
        let cbs_to_gm_ident: std::collections::HashMap<u16, String> = d
            .municipality
            .iter()
            .map(|(g, _, _)| (num(g), format!("Gm{}", to_ident(&g.name))))
            .collect();
        for ws in staat
            .subregions
            .iter()
            .filter(of(RegionCategory::WaterAuthority))
        {
            let ws_num = num(ws);
            d.water_authority.push(ws.clone());
            for wsk in ws
                .subregions
                .iter()
                .filter(of(RegionCategory::WaterAuthorityElectoralDistrict))
            {
                d.water_authority_electoral_district
                    .push((wsk.clone(), ws_num));
                for gm in wsk
                    .subregions
                    .iter()
                    .filter(of(RegionCategory::WaterAuthorityMunicipality))
                {
                    if let Some(gm_ident) = cbs_to_gm_ident.get(&num(gm)) {
                        d.water_authority_municipality.push((
                            gm.clone(),
                            gm_ident.clone(),
                            ws_num,
                            gm.frysian_export_allowed,
                        ));
                    }
                }
            }
        }

        // Sort every collection except for municipalities by its RegionNumber,
        // so ordering doesn't depend on the document order of MasterElectionTree.xml
        d.provinces_and_colleges.sort_by_key(num);
        d.province_polling_stations.sort_by_key(|(r, _)| num(r));
        d.electoral_districts.sort_by_key(num);
        d.province_electoral_district.sort_by_key(|(r, _)| num(r));
        d.water_authority.sort_by_key(num);
        d.water_authority_electoral_district
            .sort_by_key(|(r, _)| num(r));

        d
    }
}

/// Shared test fixtures used by both this module's tests and the `codegen`
/// submodule tests.
#[cfg(test)]
pub(crate) mod test_support {
    use eml_nl::documents::master_election_tree::MetCommittee;

    use super::*;

    pub(crate) fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eks-utils-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn test_districts() -> Districts {
        Districts {
            provinces_and_colleges: vec![
                MetRegion::new("Provincie-A", RegionCategory::Province)
                    .with_number(1)
                    .with_committees(vec![MetCommittee::new(
                        eml_nl::utils::ElectionCategory::EK,
                        eml_nl::utils::CommitteeCategory::CSB,
                    )]),
                MetRegion::new("Kiescollege-B", RegionCategory::ElectoralCollege).with_number(99),
            ],
            province_polling_stations: vec![
                (
                    MetRegion::new("Stembureau-A", RegionCategory::ProvincePollingStation),
                    1,
                ),
                (
                    MetRegion::new("Stembureau-B", RegionCategory::ProvincePollingStation)
                        .with_number(2),
                    99,
                ),
            ],
            electoral_districts: vec![MetRegion::new(
                "Kieskring-A",
                RegionCategory::ElectoralDistrict,
            )],
            province_electoral_district: vec![(
                MetRegion::new(
                    "Provinciaal-Kieskring-A",
                    RegionCategory::ProvinceElectoralDistrict,
                )
                .with_number(2),
                1,
            )],
            municipality: vec![(
                MetRegion::new("Gemeente-A", RegionCategory::Municipality).with_number(1),
                2,
                1,
            )],
            water_authority: vec![
                MetRegion::new("Waterschap-A", RegionCategory::WaterAuthority).with_number(3),
            ],
            water_authority_electoral_district: vec![(
                MetRegion::new(
                    "Waterschap-Kieskring-A",
                    RegionCategory::WaterAuthorityElectoralDistrict,
                ),
                3,
            )],
            water_authority_municipality: vec![
                (
                    MetRegion::new(
                        "Waterschap-Gemeente-A",
                        RegionCategory::WaterAuthorityMunicipality,
                    ),
                    "GmGemeenteA".to_string(),
                    3,
                    false,
                ),
                (
                    MetRegion::new(
                        "Waterschap-Gemeente-B",
                        RegionCategory::WaterAuthorityMunicipality,
                    ),
                    "GmGemeenteB".to_string(),
                    3,
                    true,
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::districts::test_support::test_districts;

    #[test]
    fn districts_are_of_correct_category() {
        let districts = Districts::parse_from_file(Path::new("../../MasterElectionTree.xml"));
        assert!(
            districts
                .provinces_and_colleges
                .iter()
                .all(|d| d.key.category == RegionCategory::Province
                    || d.key.category == RegionCategory::ElectoralCollege)
        );
        assert!(
            districts
                .province_polling_stations
                .iter()
                .all(|(d, _)| d.key.category == RegionCategory::ProvincePollingStation)
        );
        assert!(
            districts
                .electoral_districts
                .iter()
                .all(|d| d.key.category == RegionCategory::ElectoralDistrict)
        );
        assert!(
            districts
                .province_electoral_district
                .iter()
                .all(|(d, _)| d.key.category == RegionCategory::ProvinceElectoralDistrict)
        );
        assert!(
            districts
                .municipality
                .iter()
                .all(|(d, _, _)| d.key.category == RegionCategory::Municipality)
        );
        assert!(
            districts
                .water_authority
                .iter()
                .all(|d| d.key.category == RegionCategory::WaterAuthority)
        );
        assert!(
            districts
                .water_authority_electoral_district
                .iter()
                .all(|(d, _)| d.key.category == RegionCategory::WaterAuthorityElectoralDistrict)
        );
        assert!(
            districts
                .water_authority_municipality
                .iter()
                .all(|(d, _, _, _)| d.key.category == RegionCategory::WaterAuthorityMunicipality)
        );
    }

    #[test]
    fn all_entries_contains_every_district() {
        let districts = test_districts();
        let entries = districts.all_entries();
        let variants: std::collections::HashSet<&str> =
            entries.iter().map(|e| e.variant.as_str()).collect();

        assert!(variants.contains("ProvincieA"));
        assert!(variants.contains("KiescollegeB"));
        assert!(variants.contains("SbStembureauA"));
        assert!(variants.contains("SbStembureauB"));
        assert!(variants.contains("TkKieskringA"));
        assert!(variants.contains("PsProvinciaalKieskringA"));
        assert!(variants.contains("GmGemeenteA"));
        assert!(variants.contains("GmGemeenteBFr"));
        assert!(variants.contains("WsWaterschapKieskringA"));

        // The "GmGemeenteA" is both a water authority municipality and a normal
        // municipality, but it should not produce a duplicate entry
        assert_eq!(
            entries.len(),
            9,
            "all_entries should have one entry per district, without duplicates"
        );
    }
}
