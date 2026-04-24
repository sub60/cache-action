use core::convert::Infallible;

use futures::AsyncRead;
use nix_types::{NarInfo, NarInfoFileName};

use crate::context::Cache;
use crate::protocol::StoreDir;

#[derive(Default, Clone, Copy)]
pub(crate) struct NoopCache {}

impl Cache for NoopCache {
    type NarUploadState = ();
    type Error = Infallible;

    async fn has_narinfo(
        &self,
        _: &NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn initiate_nar_upload(
        &self,
        _: &NarInfoFileName,
    ) -> Result<Self::NarUploadState, Self::Error> {
        Ok(())
    }

    async fn upload_nar(
        &self,
        (): &mut Self::NarUploadState,
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
        (): Self::NarUploadState,
    ) -> Result<u64, Self::Error> {
        Ok(narinfo.with_url("").to_string().len() as u64)
    }
}
