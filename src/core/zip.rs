use async_zip::{
    Compression, ZipDateTime, ZipDateTimeBuilder, ZipEntryBuilder, tokio::write::ZipFileWriter,
};
use jiff::Timestamp;
use tokio::io::AsyncWrite;

use crate::{AppError, constants::DEFAULT_TIMEZONE};

/// Regular file, `rw-r--r--`
const FILE_MODE: u16 = 0o100_644;

/// Writer used to add files into a streaming ZIP archive.
pub struct ZipResponseWriter<W: AsyncWrite + Unpin> {
    inner: ZipFileWriter<W>,
    modified: ZipDateTime,
}

impl<W: AsyncWrite + Unpin> ZipResponseWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            inner: ZipFileWriter::with_tokio(writer),
            modified: zip_date_time(Timestamp::now()),
        }
    }

    pub async fn add_file(&mut self, name: &str, data: &[u8]) -> Result<(), AppError> {
        let builder = ZipEntryBuilder::new(name.into(), Compression::Deflate)
            .unix_permissions(FILE_MODE)
            .last_modification_date(self.modified);

        self.inner
            .write_entry_whole(builder, data)
            .await
            .map_err(|_| AppError::InternalServerError)
    }

    pub async fn finish(self) -> Result<(), AppError> {
        self.inner
            .close()
            .await
            .map(|_| ())
            .map_err(|_| AppError::InternalServerError)
    }
}

/// `now` in Dutch local time; ZIP timestamps carry no time zone
fn zip_date_time(now: Timestamp) -> ZipDateTime {
    let local = now.to_zoned_datetime(DEFAULT_TIMEZONE.clone());

    // Month, day and time fields are never negative.
    let field = |value: i8| u32::from(value.unsigned_abs());

    ZipDateTimeBuilder::new()
        .year(i32::from(local.year()))
        .month(field(local.month()))
        .day(field(local.day()))
        .hour(field(local.hour()))
        .minute(field(local.minute()))
        .second(field(local.second()))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_zip::base::read1::seek::ZipArchiveReader;
    use futures_lite::io::Cursor;

    #[test]
    fn zip_date_time_is_dutch_local_time() {
        let date = zip_date_time("2027-03-18T23:30:10Z".parse().unwrap());

        assert_eq!(
            (date.year(), date.month(), date.day()),
            (2027, 3, 19),
            "23:30 UTC is past midnight in Amsterdam"
        );
        assert_eq!((date.hour(), date.minute(), date.second()), (0, 30, 10));
    }

    /// Entries are readable files with a real date, not mode `0000` dated
    /// `1980-00-00`.
    #[tokio::test]
    async fn entries_are_readable_and_dated() -> Result<(), AppError> {
        let mut bytes = Vec::new();
        let mut zipper = ZipResponseWriter::new(&mut bytes);
        zipper.add_file("a.txt", b"a").await?;
        zipper.finish().await?;

        let zip = ZipArchiveReader::open(Cursor::new(bytes))
            .await
            .expect("zip");
        let header = &zip.cdrs()[0].cdrh;
        assert_eq!(header.exter_attr >> 16, u32::from(FILE_MODE));
        // DOS dates count the years since 1980 in the top 7 bits
        assert!(1980 + (header.mod_date >> 9) >= 2026);

        Ok(())
    }
}
