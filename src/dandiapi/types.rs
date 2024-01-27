use super::{DandisetId, VersionId};
use crate::paths::{PureDirPath, PurePath};
use crate::s3::{PrefixedS3Client, S3Entry, S3Folder, S3Location, S3Object};
use serde::Deserialize;
use thiserror::Error;
use time::OffsetDateTime;
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(super) struct Page<T> {
    pub(super) next: Option<Url>,
    pub(super) results: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct Dandiset {
    pub(crate) identifier: DandisetId,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) modified: OffsetDateTime,
    //contact_person: String,
    //embargo_status: ...,
    pub(crate) draft_version: DandisetVersion,
    pub(crate) most_recent_published_version: Option<DandisetVersion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct DandisetVersion {
    pub(crate) version: VersionId,
    //name: String,
    //asset_count: u64,
    pub(crate) size: i64,
    //status: ...,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) modified: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VersionMetadata(pub(super) Vec<u8>);

impl VersionMetadata {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<VersionMetadata> for Vec<u8> {
    fn from(value: VersionMetadata) -> Vec<u8> {
        value.0
    }
}

// Item in a `/dandisets/{dandiset_id}/versions/{version_id}/assets/paths`
// response
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(from = "RawFolderEntry")]
pub(crate) enum FolderEntry {
    Folder(AssetFolder),
    Asset { path: PurePath, id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AssetFolder {
    pub(crate) path: PureDirPath,
}

impl From<RawFolderEntry> for FolderEntry {
    fn from(entry: RawFolderEntry) -> FolderEntry {
        if let Some(asset) = entry.asset {
            FolderEntry::Asset {
                path: entry.path,
                id: asset.asset_id,
            }
        } else {
            FolderEntry::Folder(AssetFolder {
                path: entry.path.to_dir_path(),
            })
        }
    }
}

// Raw item in a `/dandisets/{dandiset_id}/versions/{version_id}/assets/paths`
// response
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct RawFolderEntry {
    path: PurePath,
    asset: Option<RawFolderEntryAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct RawFolderEntryAsset {
    asset_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AtAssetPath {
    Folder(AssetFolder),
    Asset(Asset),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "RawAsset")]
pub(crate) enum Asset {
    Blob(BlobAsset),
    Zarr(ZarrAsset),
}

impl Asset {
    pub(crate) fn path(&self) -> &PurePath {
        match self {
            Asset::Blob(a) => &a.path,
            Asset::Zarr(a) => &a.path,
        }
    }

    pub(crate) fn size(&self) -> i64 {
        match self {
            Asset::Blob(a) => a.size,
            Asset::Zarr(a) => a.size,
        }
    }

    pub(crate) fn created(&self) -> OffsetDateTime {
        match self {
            Asset::Blob(a) => a.created,
            Asset::Zarr(a) => a.created,
        }
    }

    pub(crate) fn modified(&self) -> OffsetDateTime {
        match self {
            Asset::Blob(a) => a.modified,
            Asset::Zarr(a) => a.modified,
        }
    }

    pub(crate) fn metadata(&self) -> &AssetMetadata {
        match self {
            Asset::Blob(a) => &a.metadata,
            Asset::Zarr(a) => &a.metadata,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BlobAsset {
    pub(crate) asset_id: String,
    pub(crate) blob_id: String,
    pub(crate) path: PurePath,
    pub(crate) size: i64,
    pub(crate) created: OffsetDateTime,
    pub(crate) modified: OffsetDateTime,
    pub(crate) metadata: AssetMetadata,
}

impl BlobAsset {
    pub(crate) fn content_type(&self) -> Option<&str> {
        self.metadata.encoding_format.as_deref()
    }

    pub(crate) fn etag(&self) -> Option<&str> {
        self.metadata.digest.dandi_etag.as_deref()
    }

    pub(crate) fn download_url(&self) -> Option<&Url> {
        self.metadata
            .content_url
            .iter()
            .find(|url| S3Location::parse_url(url).is_ok())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ZarrAsset {
    pub(crate) asset_id: String,
    pub(crate) zarr_id: String,
    pub(crate) path: PurePath,
    pub(crate) size: i64,
    pub(crate) created: OffsetDateTime,
    pub(crate) modified: OffsetDateTime,
    pub(crate) metadata: AssetMetadata,
}

impl ZarrAsset {
    pub(crate) fn s3location(&self) -> Option<S3Location> {
        self.metadata
            .content_url
            .iter()
            .find_map(|url| S3Location::parse_url(url).ok())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssetMetadata {
    encoding_format: Option<String>,
    content_url: Vec<Url>,
    digest: AssetDigests,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct AssetDigests {
    #[serde(rename = "dandi:dandi-etag")]
    dandi_etag: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct RawAsset {
    asset_id: String,
    blob: Option<String>,
    zarr: Option<String>,
    path: PurePath,
    size: i64,
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    modified: OffsetDateTime,
    metadata: AssetMetadata,
}

impl TryFrom<RawAsset> for Asset {
    type Error = AssetTypeError;

    fn try_from(value: RawAsset) -> Result<Asset, AssetTypeError> {
        match (value.blob, value.zarr) {
            (Some(blob_id), None) => Ok(Asset::Blob(BlobAsset {
                asset_id: value.asset_id,
                blob_id,
                path: value.path,
                size: value.size,
                created: value.created,
                modified: value.modified,
                metadata: value.metadata,
            })),
            (None, Some(zarr_id)) => Ok(Asset::Zarr(ZarrAsset {
                asset_id: value.asset_id,
                zarr_id,
                path: value.path,
                size: value.size,
                created: value.created,
                modified: value.modified,
                metadata: value.metadata,
            })),
            (None, None) => Err(AssetTypeError::Neither {
                asset_id: value.asset_id,
            }),
            (Some(_), Some(_)) => Err(AssetTypeError::Both {
                asset_id: value.asset_id,
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(crate) enum AssetTypeError {
    #[error(r#"asset {asset_id} has neither "blob" nor "zarr" set"#)]
    Neither { asset_id: String },
    #[error(r#"asset {asset_id} has both "blob" and "zarr" set"#)]
    Both { asset_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DandiResource {
    Folder(AssetFolder),
    Asset(Asset),
    ZarrFolder(ZarrFolder),
    ZarrEntry(ZarrEntry),
}

impl DandiResource {
    pub(super) fn with_s3(self, s3: PrefixedS3Client) -> DandiResourceWithS3 {
        match self {
            DandiResource::Folder(r) => DandiResourceWithS3::Folder(r),
            DandiResource::Asset(r) => DandiResourceWithS3::Asset(r),
            DandiResource::ZarrFolder(folder) => DandiResourceWithS3::ZarrFolder { folder, s3 },
            DandiResource::ZarrEntry(r) => DandiResourceWithS3::ZarrEntry(r),
        }
    }
}

impl From<S3Entry> for DandiResource {
    fn from(value: S3Entry) -> DandiResource {
        match value {
            S3Entry::Folder(folder) => DandiResource::ZarrFolder(folder.into()),
            S3Entry::Object(obj) => DandiResource::ZarrEntry(obj.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ZarrFolder {
    pub(crate) path: PureDirPath,
}

impl From<S3Folder> for ZarrFolder {
    fn from(value: S3Folder) -> ZarrFolder {
        ZarrFolder {
            path: value.key_prefix,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ZarrEntry {
    pub(crate) path: PurePath,
    pub(crate) size: i64,
    pub(crate) modified: OffsetDateTime,
    pub(crate) etag: String,
    pub(crate) url: Url,
}

impl From<S3Object> for ZarrEntry {
    fn from(obj: S3Object) -> ZarrEntry {
        ZarrEntry {
            path: obj.key,
            size: obj.size,
            modified: obj.modified,
            etag: obj.etag,
            url: obj.download_url,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum DandiResourceWithS3 {
    Folder(AssetFolder),
    Asset(Asset),
    ZarrFolder {
        folder: ZarrFolder,
        s3: PrefixedS3Client,
    },
    ZarrEntry(ZarrEntry),
}

impl From<AtAssetPath> for DandiResourceWithS3 {
    fn from(value: AtAssetPath) -> DandiResourceWithS3 {
        match value {
            AtAssetPath::Folder(r) => DandiResourceWithS3::Folder(r),
            AtAssetPath::Asset(r) => DandiResourceWithS3::Asset(r),
        }
    }
}

impl From<DandiResourceWithS3> for DandiResource {
    fn from(value: DandiResourceWithS3) -> DandiResource {
        match value {
            DandiResourceWithS3::Folder(r) => DandiResource::Folder(r),
            DandiResourceWithS3::Asset(r) => DandiResource::Asset(r),
            DandiResourceWithS3::ZarrFolder { folder, .. } => DandiResource::ZarrFolder(folder),
            DandiResourceWithS3::ZarrEntry(r) => DandiResource::ZarrEntry(r),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DandiResourceWithChildren {
    Folder {
        folder: AssetFolder,
        children: Vec<DandiResource>,
    },
    Blob(BlobAsset),
    Zarr {
        zarr: ZarrAsset,
        children: Vec<DandiResource>,
    },
    ZarrFolder {
        folder: ZarrFolder,
        children: Vec<DandiResource>,
    },
    ZarrEntry(ZarrEntry),
}
