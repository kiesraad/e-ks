//! Locality names that are not the official BAG spelling.
//!
//! A misspelling ("Den Haag") is always corrected. A Frisian locality has two
//! real names ("Berltsum" / "Berlikum"); both are accepted, neither rewritten.

/// Misspellings and the official name each stands for.
const LOCALITY_MISSPELLINGS: &[(&str, &str)] = &[
    ("Den Haag", "'s-Gravenhage"),
    ("Den Bosch", "'s-Hertogenbosch"),
    ("Gorkum", "Gorinchem"),
    ("Gorcum", "Gorinchem"),
    ("Gravendeel", "'s-Gravendeel"),
    ("Graveland", "'s-Graveland"),
    ("Gravenzande", "'s-Gravenzande"),
    ("Heer Abtskerke", "'s-Heer Abtskerke"),
    ("Heer Arendskerke", "'s-Heer Arendskerke"),
    ("Heer Hendrikskinderen", "'s-Heer Hendrikskinderen"),
    ("Heerenberg", "'s-Heerenberg"),
    ("Heerenbroek", "'s-Heerenbroek"),
    ("Heerenhoek", "'s-Heerenhoek"),
    ("St. Agatha", "Sint Agatha"),
    ("St. Annaland", "Sint-Annaland"),
    ("St. Anthonis", "Sint Anthonis"),
    ("St. Geertruid", "Sint Geertruid"),
    ("St. Hubert", "Sint Hubert"),
    ("St. Jansklooster", "Sint Jansklooster"),
    ("St. Joost", "Sint Joost"),
    ("St. Kruis", "Sint Kruis"),
    ("St. Maarten", "Sint Maarten"),
    ("St. Maartensdijk", "Sint-Maartensdijk"),
    ("St. Michielsgestel", "Sint-Michielsgestel"),
    ("St. Nicolaasga", "Sint Nicolaasga"),
    ("St. Odiliënberg", "Sint Odiliënberg"),
    ("St. Oedenrode", "Sint-Oedenrode"),
    ("St. Pancras", "Sint Pancras"),
    ("St. Philipsland", "Sint Philipsland"),
    ("Sint Willebrord", "St. Willebrord"),
    ("Cuyk", "Cuijk"),
];

/// Dutch and official Frisian name of the same locality, based on
/// <https://cuatro.sim-cdn.nl/fryslan/uploads/list_plaknammen_yn_fryslan.pdf?cb=RAqKiBSg>
const FRISIAN_ALIASES: &[(&str, &str)] = &[
    ("Oude Leije", "Alde Leie"),
    ("Oldeboorn", "Aldeboarn"),
    ("Oudkerk", "Aldtsjerk"),
    ("Oudwoude", "Aldwâld"),
    ("Augsbuurt", "Augsbuert-Lytsewâld"),
    ("Baijum", "Baaium"),
    ("Beers", "Bears"),
    ("Berlikum", "Berltsum"),
    ("Beetgum", "Bitgum"),
    ("Beetgumermolen", "Bitgummole"),
    ("Blija", "Blije"),
    ("Bornwird", "Boarnwert"),
    ("Bozum", "Boazum"),
    ("Britswerd", "Britswert"),
    ("Broeksterwoude", "Broeksterwâld"),
    ("Birdaard", "Burdaard"),
    ("Bergum", "Burgum"),
    ("Damwoude", "Damwâld"),
    ("De Valom", "De Falom"),
    ("Triemen", "De Trieme"),
    ("Zwaagwesteinde", "De Westereen"),
    ("Deersum", "Dearsum"),
    ("Driesum", "Driezum"),
    ("Dronrijp", "Dronryp"),
    ("Aegum", "Eagum"),
    ("Anjum", "Eanjum"),
    ("Eernewoude", "Earnewâld"),
    ("Oosterend", "Easterein"),
    ("Oosterlittens", "Easterlittens"),
    ("Oostermeer", "Eastermar"),
    ("Oosternijkerk", "Easternijtsjerk"),
    ("Oosterwierum", "Easterwierrum"),
    ("Oostrum", "Eastrum"),
    ("Veenklooster", "Feankleaster"),
    ("Veenwouden", "Feanwâlden"),
    ("Veenwoudsterwal", "Feanwâldsterwâl"),
    ("Finkum", "Feinsum"),
    ("Ferwerd", "Ferwert"),
    ("Garijp", "Garyp"),
    ("Genum", "Ginnum"),
    ("Grouw", "Grou"),
    ("Giekerk", "Gytsjerk"),
    ("Hantumeruitburen", "Hantumerútbuorren"),
    ("Hantumhuizen", "Hantumhuzen"),
    ("Hogebeintum", "Hegebeintum"),
    ("Hijlaard", "Hilaard"),
    ("Hennaard", "Hinnaard"),
    ("Holwerd", "Holwert"),
    ("Huins", "Húns"),
    ("Hardegarijp", "Hurdegaryp"),
    ("Idaard", "Idaerd"),
    ("Ee", "Ie"),
    ("Edens", "Iens"),
    ("Engelum", "Ingelum"),
    ("Engwierum", "Ingwierrum"),
    ("Het Heidenschap", "It Heidenskip"),
    ("Janum", "Jannum"),
    ("Irnsum", "Jirnsum"),
    ("Eestrum", "Jistrum"),
    ("Jonkersland", "Jonkerslân"),
    ("Jorwerd", "Jorwert"),
    ("Cornjum", "Koarnjum"),
    ("Kollumerzwaag", "Kollumersweach"),
    ("Kubaard", "Kûbaard"),
    ("Lions", "Leons"),
    ("Lioessens", "Ljussens"),
    ("Lutkewierum", "Lytsewierrum"),
    ("Marssum", "Marsum"),
    ("Menaldum", "Menaam"),
    ("Metslawier", "Mitselwier"),
    ("Morra", "Moarre"),
    ("Molenend", "Mûnein"),
    ("Niawier", "Nijewier"),
    ("Noordbergum", "Noardburgum"),
    ("Oenkerk", "Oentsjerk"),
    ("Paesens", "Peazens"),
    ("Poppingawier", "Poppenwier"),
    ("Rauwerd", "Raerd"),
    ("Roodkerk", "Readtsjerk"),
    ("Roodhuis", "Reahûs"),
    ("Roordahuizum", "Reduzum"),
    ("Rinsumageest", "Rinsumageast"),
    ("Rijperkerk", "Ryptsjerk"),
    ("Sijbrandaburen", "Sibrandabuorren"),
    ("Sijbrandahuis", "Sibrandahûs"),
    ("Schingen", "Skingen"),
    ("Suameer", "Sumar"),
    ("Suawoude", "Suwâld"),
    ("Zwagerbosch", "Sweagerbosk"),
    ("Terhorne", "Terherne"),
    ("Terzool", "Tersoal"),
    ("Tietjerk", "Tytsjerk"),
    ("Waaxens", "Waaksens"),
    ("Wouterswoude", "Wâlterswâld"),
    ("Wanswerd", "Wânswert"),
    ("Wartena", "Warten"),
    ("Warga", "Wergea"),
    ("Westergeest", "Westergeast"),
    ("Wieuwerd", "Wiuwert"),
    ("Welsrijp", "Wjelsryp"),
    ("Wijns", "Wyns"),
    ("Wijtgaard", "Wytgaard"),
    ("IJsbrechtum", "Ysbrechtum"),
];

/// The official name for a misspelling, or `None` when `input` is not one.
pub fn correct_locality_name(input: &str) -> Option<&'static str> {
    find(LOCALITY_MISSPELLINGS, input)
}

/// The listed spelling of a Frisian locality's Dutch or Frisian name.
pub fn frisian_locality_name(input: &str) -> Option<&'static str> {
    let input = input.to_lowercase();
    FRISIAN_ALIASES
        .iter()
        .flat_map(|(dutch, frisian)| [dutch, frisian])
        .find(|name| name.to_lowercase() == input)
        .copied()
}

/// Whether two names denote the same place: equal, or a Frisian pair.
pub fn same_locality(one: &str, other: &str) -> bool {
    let (one, other) = (one.to_lowercase(), other.to_lowercase());
    if one == other {
        return true;
    }

    let frisian_name = |name: &str| find(FRISIAN_ALIASES, name).map(str::to_lowercase);
    frisian_name(&one).as_ref() == Some(&other) || frisian_name(&other).as_ref() == Some(&one)
}

fn find(table: &[(&str, &'static str)], input: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(name, _)| name.to_lowercase() == input.to_lowercase())
        .map(|(_, official)| *official)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::bag;

    #[test]
    fn corrects_a_misspelled_name() {
        assert_eq!(correct_locality_name("Den Haag"), Some("'s-Gravenhage"));
        assert_eq!(correct_locality_name("den haag"), Some("'s-Gravenhage"));
        assert_eq!(correct_locality_name("Cuyk"), Some("Cuijk"));
    }

    #[test]
    fn leaves_a_name_that_is_not_a_misspelling_alone() {
        assert_eq!(correct_locality_name("'s-Gravenhage"), None);
        assert_eq!(correct_locality_name("Amsterdam"), None);
    }

    #[test]
    fn never_corrects_between_dutch_and_frisian() {
        assert_eq!(correct_locality_name("Berlikum"), None);
        assert_eq!(correct_locality_name("Berltsum"), None);
        assert_eq!(correct_locality_name("Oosterend"), None);
    }

    /// Correcting a name the BAG uses would move someone to another place.
    #[test]
    fn no_misspelling_is_an_official_locality() {
        for (misspelling, official) in LOCALITY_MISSPELLINGS {
            assert_eq!(
                bag::official_locality_name(misspelling),
                None,
                "{misspelling:?} is a locality of its own, so correcting it to {official:?} is wrong"
            );
        }
    }

    #[test]
    fn frisian_locality_name_accepts_either_spelling() {
        assert_eq!(frisian_locality_name("berlikum"), Some("Berlikum"));
        assert_eq!(frisian_locality_name("berltsum"), Some("Berltsum"));
        assert_eq!(frisian_locality_name("Amsterdam"), None);
        assert_eq!(frisian_locality_name("Den Haag"), None);
    }

    #[test]
    fn same_locality_accepts_either_language() {
        assert!(same_locality("Berltsum", "Berlikum"));
        assert!(same_locality("Berlikum", "Berltsum"));
        assert!(same_locality("Amsterdam", "amsterdam"));

        assert!(!same_locality("Amsterdam", "Rotterdam"));
        assert!(!same_locality("Berltsum", "Bitgum"));
        // A misspelling is not an accepted second name for its place.
        assert!(!same_locality("'s-Gravenhage", "Den Haag"));
    }
}
