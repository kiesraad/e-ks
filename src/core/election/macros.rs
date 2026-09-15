/// Macro to define election configs, used in configs.rs
macro_rules! define_elections {
    (
        $(
            $name:ident $( ( $binding:ident : $binding_ty:ty ) )? {
                election_type: $election_type:expr,
                titles: {
                    nl: $title_nl:expr,
                    fry: $title_fry:expr,
                    en: $title_en:expr $(,)?
                },
                electoral_districts: $electoral_districts:expr,
                number_of_seats: $number_of_seats:expr,
                eligible_date_of_birth: $eligible_date_of_birth:expr,
                nomination_day_date: $nomination_day_date:expr,
                document_review_date: $document_review_date:expr,
                omission_period_end_date: $omission_period_end_date:expr,
                public_session: $public_session:expr,
                election_date: $election_date:expr
            }
        ),* $(,)?
    ) => {
	    /// Active election configurations and ruleset for the application.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum ElectionConfig {
            $(
                $name $(($binding_ty))?,
            )*
        }

        impl ElectionConfig {
            /// Short code identifying the election type (without region), used in forms.
            pub fn code(&self) -> &'static str {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => stringify!($name),
                    )*
                }
            }

            /// Returns the region code (province or water council code), if any.
            pub fn region_code(&self) -> Option<&'static str> {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => {
                            #[allow(unused_mut, unused_assignments)]
                            let mut result: Option<&'static str> = None;
                            $( result = Some($binding.code()); )?
                            result
                        },
                    )*
                }
            }

            /// Returns the region title (province or water council name), if any.
            pub fn region_title(&self) -> Option<&'static str> {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => {
                            #[allow(unused_mut, unused_assignments)]
                            let mut result: Option<&'static str> = None;
                            $( result = Some($binding.title()); )?
                            result
                        },
                    )*
                }
            }

            /// Returns the region number, if any.
            pub fn region_number(&self) -> Option<u16> {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => {
                            #[allow(unused_mut, unused_assignments)]
                            let mut result: Option<u16> = None;
                            $( result = Some($binding.region_number()); )?
                            result
                        },
                    )*
                }
            }

            pub fn election_type(&self) -> ElectionType {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $election_type,
                    )*
                }
            }

            pub fn title(&self, locale: AnyLocale) -> &'static str {
                #[allow(unused)]
                match (self, locale) {
                    $(
                        (Self::$name $(($binding))?, AnyLocale::Nl) => $title_nl,
                        (Self::$name $(($binding))?, AnyLocale::Fry) => $title_fry,
                        (Self::$name $(($binding))?, AnyLocale::En) => $title_en,
                    )*
                }
            }


            pub fn nomination_day_date(&self) -> NaiveDate {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $nomination_day_date,
                    )*
                }
            }

            pub fn election_date(&self) -> NaiveDate {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $election_date,
                    )*
                }
            }

            pub fn eligible_date_of_birth(&self) -> NaiveDate {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $eligible_date_of_birth,
                    )*
                }
            }

            pub fn electoral_districts(&self) -> &'static [ElectoralDistrict] {
                match self {
                    $(
                        Self::$name $(($binding))? => $electoral_districts,
                    )*
                }
            }

            pub fn number_of_seats(&self) -> u64 {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $number_of_seats,
                    )*
                }
            }

            pub fn frisian_export_allowed(&self) -> bool {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => {
                            #[allow(unused_mut, unused_assignments)]
                            let mut result = false;
                            $( result = $binding.frisian_export_allowed(); )?
                            result
                        },
                    )*
                }
            }

            pub fn document_review_date(&self) -> NaiveDate {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $document_review_date,
                    )*
                }
            }

            pub fn omission_period_end_date(&self) -> NaiveDate {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $omission_period_end_date,
                    )*
                }
            }

            pub fn public_session(&self) -> crate::core::election::PublicSession {
                #[allow(unused)]
                match self {
                    $(
                        Self::$name $(($binding))? => $public_session,
                    )*
                }
            }

            /// Parse an election code plus optional region code into a variant.
            /// Variants without a region ignore the `region` argument; variants
            /// with one return `None` if `region` is missing or invalid.
            #[allow(unused_variables)]
            pub fn from_code_and_region(code: &str, region: Option<&str>) -> Option<Self> {
                $(
                    if code == stringify!($name) {
                        return Some(Self::$name $((<$binding_ty>::from_code(region?)?))?);
                    }
                )*
                None
            }

        }
    };
}

pub(crate) use define_elections;
