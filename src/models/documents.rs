//! The submit-documents download: collects the store data for a candidate
//! list and streams the rendered PDF models plus the EML 2.10 nomination
//! export as a ZIP response.

use super::{
    Pdf,
    eml::eml210::eml210,
    h1::H1,
    h3::H3,
    h4::H4,
    h9::H9,
    inputs::{
        DetailedCandidate, ElectoralDistricts, ModelData, NameAuthorisation, Person,
        ordered_candidates,
    },
};
use crate::{
    AppError, Context, ElectionConfig, PgStore,
    core::{ModelLocale, ZipResponseWriter},
    structs::{
        candidate_lists::{CandidateListId, FullCandidateList},
        common::{HasSeverity, Problematic, Severity},
        list_designation::ListDesignation,
    },
    utils::{format_hash, no_cache_headers},
};
use axum::{
    body::Body,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use eks_utils::slugify_teletex;
use tokio::io::duplex;
use tokio_util::io::ReaderStream;
use tracing::error;

pub const ZIP_CONTENT_TYPE: &str = "application/zip";

pub struct DocumentData {
    pub list_id: CandidateListId,
    pub folder_name: Option<String>,
    pub election: ElectionConfig,
    pub model_data: ModelData,
    pub electoral_districts: ElectoralDistricts,
    pub detailed_candidates: Vec<DetailedCandidate>,
    pub previously_seated: bool,
    pub list_designation: ListDesignation,
    pub list_submitter: Person,
    pub substitute_submitters: Vec<Person>,
    pub name_authorisations: Vec<NameAuthorisation>,
    /// The EML 2.10 nomination XML, built eagerly so errors surface before
    /// the ZIP starts streaming.
    nomination: Vec<u8>,
}

impl DocumentData {
    pub fn archive_filename(&self) -> String {
        let election_slug = self.election.filename_slug();
        let version = self.model_data.event_id;

        let name_slug = if self.list_designation == ListDesignation::Blank {
            "blanco".to_string()
        } else {
            slugify_teletex(&self.model_data.appellation, true)
        };

        if self.model_data.locale == ModelLocale::Fry {
            format!("{name_slug}-{election_slug}-v{version}-fry.zip")
        } else {
            format!("{name_slug}-{election_slug}-v{version}.zip")
        }
    }

    /// Get a list of `NameAuthorisation` with the right number of authorisations based on
    /// the type of list designation:
    ///
    /// - Blank lists always have 0 name authorisations -> No H3-1 or H3-2
    /// - Combined lists have at least 2 name authorisations -> H3-2
    /// - Standalone lists always have 1 name authorisation -> H3-1
    ///
    /// If there are fewer name authorisations than required, we add fill-ins that show up as
    /// empty spaces on the models.
    fn name_authorisations_with_fill_ins(
        store: &PgStore,
    ) -> Result<Vec<NameAuthorisation>, AppError> {
        let name_authorisations = store.get_name_authorisations();

        match store.get_political_group().list_designation {
            Some(ListDesignation::Blank) => Ok(Vec::new()),
            Some(ListDesignation::Combined) => {
                let mut auths: Vec<NameAuthorisation> =
                    name_authorisations.iter().map(Into::into).collect();

                while auths.len() < 2 {
                    auths.push(NameAuthorisation::default());
                }

                Ok(auths)
            }
            _ => {
                if name_authorisations.len() > 1 {
                    return Err(AppError::IncompleteData(
                        "Expected no more than 1 name authorisation",
                    ));
                }

                let auth = name_authorisations
                    .first()
                    .map(Into::into)
                    .unwrap_or_default();

                Ok(vec![auth])
            }
        }
    }

    /// Collect all the necessary data to render the models and the exported EML.
    ///
    /// Collecting the data first prevents errors popping up while the ZIP is streaming,
    /// and it is more efficient because we only collect everything once.
    pub fn new(
        store: &PgStore,
        context: &Context,
        list_id: CandidateListId,
        locale: ModelLocale,
    ) -> Result<Self, AppError> {
        let election = context.election;
        if !election.frisian_export_allowed() && locale == ModelLocale::Fry {
            return Err(AppError::UserError(
                "Frisian export not allowed for this election".to_string(),
            ));
        }

        let event_id = store.current_event_id();
        let event_hash = store.current_event_hash();

        let FullCandidateList { list, candidates } = FullCandidateList::get(store, list_id)?;
        let mut candidates = candidates.into_iter().map(|c| c.data).collect::<Vec<_>>();

        let ordered_candidates = ordered_candidates(&mut candidates, locale)?;
        let detailed_candidates = candidates
            .iter()
            .map(|c| DetailedCandidate::try_from(c, locale))
            .collect::<Result<Vec<_>, _>>()?;

        let electoral_districts = ElectoralDistricts::from(&list, &context.election);

        let group = store.get_political_group();
        let appellation = group.pg_appellation()?;

        let list_submitter = Self::complete_list_submitter(store)?;

        let substitute_submitters = store
            .get_substitute_submitters()
            .into_iter()
            .map(Person::from)
            .collect();

        let nomination = eml210(store, &election, &group, list_id, locale)?;
        let folder_name = format!(
            "{}-{}",
            match locale {
                ModelLocale::Nl => "kieskring",
                ModelLocale::Fry => "kiesrunte",
            },
            list.districts_codes()
        );

        Ok(Self {
            list_id,
            folder_name: Some(folder_name),
            election,
            model_data: ModelData {
                election_name: election.formal_title(locale),
                election_type: election.election_type(),
                appellation,
                candidates: ordered_candidates,
                locale,
                event_id,
                sha_hash: format_hash(&event_hash, true),
            },
            electoral_districts,
            detailed_candidates,
            previously_seated: group.was_previously_seated(),
            list_designation: group.list_designation.unwrap_or_default(),
            list_submitter,
            substitute_submitters,
            name_authorisations: Self::name_authorisations_with_fill_ins(store)?,
            nomination,
        })
    }

    /// The list submitter as a model person, or an error when it is missing
    /// or incomplete.
    fn complete_list_submitter(store: &PgStore) -> Result<Person, AppError> {
        let list_submitter = store.get_list_submitter();
        if list_submitter.is_empty()
            || list_submitter
                .get_problems(())
                .has_severity_or_higher(Severity::Error)
        {
            return Err(AppError::IncompleteData("Incomplete list submitter"));
        }
        Ok(Person::from(list_submitter))
    }

    pub fn from_store_and_context(
        store: &PgStore,
        context: &Context,
        locale: ModelLocale,
    ) -> Result<(Vec<Self>, String), AppError> {
        let list_ids = store
            .get_candidate_lists()
            .into_iter()
            .map(|list| list.id)
            .collect::<Vec<_>>();

        if list_ids.is_empty() {
            return Err(AppError::IncompleteData("No candidate lists"));
        }

        let bundles = if list_ids.len() == 1 {
            let mut bundle = Self::new(store, context, list_ids[0], locale)?;
            bundle.folder_name = None;

            vec![bundle]
        } else {
            list_ids
                .iter()
                .map(|&list_id| Self::new(store, context, list_id, locale))
                .collect::<Result<Vec<_>, _>>()?
        };
        let filename = bundles[0].archive_filename();
        Ok((bundles, filename))
    }

    /// Record a document download as a `DownloadFile` audit event and stream
    /// the bundles as a zip response.
    ///
    /// The audit event is written to `event_store`. `document_store` is the
    /// (possibly historical) store the bundles were generated from; it is only
    /// used for the candidate-list count in the log line, which differs from
    /// `event_store` when serving documents for a past event.
    pub async fn serve_download(
        bundles: Vec<Self>,
        filename: String,
        download_path: String,
        event_store: &PgStore,
        document_store: &PgStore,
    ) -> Result<Response, AppError> {
        tracing::info!(
            filename,
            content_type = ZIP_CONTENT_TYPE,
            lists = document_store.get_candidate_list_count(),
            "file download served",
        );

        event_store
            .update(crate::PgEvent::DownloadFile {
                file_name: filename.clone(),
                download_path,
            })
            .await?;

        Self::to_zip_response(bundles, filename).map(IntoResponse::into_response)
    }

    pub fn to_zip_response(
        bundles: Vec<Self>,
        filename: String,
    ) -> Result<impl IntoResponse, AppError> {
        let headers = no_cache_headers::generate_attachment_headers(
            &filename,
            HeaderValue::from_static(ZIP_CONTENT_TYPE),
        )?;

        let (reader, writer) = duplex(64 * 1024);
        let body = Body::from_stream(ReaderStream::new(reader));

        tokio::spawn(async move {
            let mut zipper = ZipResponseWriter::new(writer);

            for bundle in bundles {
                let list_id = bundle.list_id;
                if let Err(err) = bundle.write_zip(&mut zipper).await {
                    error!(
                        error = ?err,
                        list_id = %list_id,
                        "failed to stream submit documents zip"
                    );
                    return;
                }
            }

            if let Err(err) = zipper.finish().await {
                error!(error = ?err, "failed to finalise submit documents zip");
            }
        });

        Ok((headers, body).into_response())
    }

    /// Render a model and add it to the zip under the given path.
    async fn add_model<T: Pdf>(
        writer: &mut ZipResponseWriter<tokio::io::DuplexStream>,
        path: &str,
        model: T,
    ) -> Result<(), AppError> {
        writer.add_file(path, &model.generate_bytes().await?).await
    }

    async fn write_zip(
        self,
        writer: &mut ZipResponseWriter<tokio::io::DuplexStream>,
    ) -> Result<(), AppError> {
        let h1 = H1 {
            common: self.model_data.clone(),
            electoral_districts: self.electoral_districts.clone(),
            previously_seated: self.previously_seated,
            list_designation: self.list_designation,
            list_submitter: self.list_submitter.clone(),
            substitute_submitters: self.substitute_submitters.clone(),
        };
        Self::add_model(writer, &self.zip_path(h1.filename()), h1).await?;

        if self.list_designation != ListDesignation::Blank {
            let h3 = H3 {
                common: self.model_data.clone(),
                electoral_districts: self.electoral_districts.clone(),
                list_designation: self.list_designation,
                list_submitter: self.list_submitter.clone(),
                name_authorisations: self.name_authorisations.clone(),
            };
            Self::add_model(writer, &self.zip_path(h3.filename()), h3).await?;
        }

        if !self.previously_seated {
            let h4 = H4 {
                common: self.model_data.clone(),
            };
            Self::add_model(writer, &self.zip_path(h4.filename()), h4).await?;
        }

        for candidate in self.detailed_candidates.iter() {
            let h9 = H9 {
                common: self.model_data.clone(),
                electoral_districts: self.electoral_districts.clone(),
                detailed_candidate: candidate.clone(),
            };
            let path = self.zip_path(format!(
                "h9-{}/{}",
                match self.model_data.locale {
                    ModelLocale::Nl => "instemmingsverklaringen",
                    ModelLocale::Fry => "ynstimmingsferklearrings",
                },
                h9.filename()
            ));
            Self::add_model(writer, &path, h9).await?;
        }

        writer
            .add_file(
                &self.zip_path("eml210.eml.xml".to_string()),
                &self.nomination,
            )
            .await?;

        Ok(())
    }

    fn zip_path(&self, relative_path: String) -> String {
        match &self.folder_name {
            Some(folder_name) => format!("{folder_name}/{relative_path}"),
            None => relative_path,
        }
    }
}
