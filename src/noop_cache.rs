use core::convert::Infallible;

use futures::AsyncRead;
use nix_types::{NarInfo, NarInfoFileName};

use crate::context::Cache;
use crate::protocol::StoreDir;

#[derive(Default, Clone, Copy)]
pub(crate) struct NoopCache {}

impl Cache for NoopCache {
    type NarUploadId = ();
    type Error = Infallible;

    async fn has_narinfo(
        &self,
        _: &NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn create_nar_upload_id(
        &self,
        _: &NarInfoFileName,
    ) -> Result<Self::NarUploadId, Self::Error> {
        Ok(())
    }

    async fn upload_nar(
        &self,
        (): &Self::NarUploadId,
        nar_bytes: impl AsyncRead + Send + 'static,
    ) -> Result<(), Self::Error> {
        let mut sink = futures::io::sink();
        futures::io::copy(nar_bytes, &mut sink).await.expect("never fails");
        Ok(())
    }

    async fn upload_narinfo(
        &self,
        _: NarInfoFileName,
        narinfo: NarInfo<(), StoreDir>,
        (): Self::NarUploadId,
    ) -> Result<u64, Self::Error> {
        Ok(narinfo.with_url("").to_string().len() as u64)
    }
}
