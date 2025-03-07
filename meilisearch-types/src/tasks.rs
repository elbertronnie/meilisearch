use std::collections::HashSet;
use std::fmt::{Display, Write};
use std::str::FromStr;

use enum_iterator::Sequence;
use milli::update::IndexDocumentsMethod;
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize, Serializer};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::{Code, ResponseError};
use crate::keys::Key;
use crate::settings::{Settings, Unchecked};
use crate::InstanceUid;

pub type TaskId = u32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub uid: TaskId,

    #[serde(with = "time::serde::rfc3339")]
    pub enqueued_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,

    pub error: Option<ResponseError>,
    pub canceled_by: Option<TaskId>,
    pub details: Option<Details>,

    pub status: Status,
    pub kind: KindWithContent,
}

impl Task {
    pub fn index_uid(&self) -> Option<&str> {
        use KindWithContent::*;

        match &self.kind {
            DumpCreation { .. }
            | SnapshotCreation
            | TaskCancelation { .. }
            | TaskDeletion { .. }
            | IndexSwap { .. } => None,
            DocumentAdditionOrUpdate { index_uid, .. }
            | DocumentDeletion { index_uid, .. }
            | DocumentClear { index_uid }
            | SettingsUpdate { index_uid, .. }
            | IndexCreation { index_uid, .. }
            | IndexUpdate { index_uid, .. }
            | IndexDeletion { index_uid } => Some(index_uid),
        }
    }

    /// Return the list of indexes updated by this tasks.
    pub fn indexes(&self) -> Vec<&str> {
        self.kind.indexes()
    }

    /// Return the content-uuid if there is one
    pub fn content_uuid(&self) -> Option<Uuid> {
        match self.kind {
            KindWithContent::DocumentAdditionOrUpdate { content_file, .. } => Some(content_file),
            KindWithContent::DocumentDeletion { .. }
            | KindWithContent::DocumentClear { .. }
            | KindWithContent::SettingsUpdate { .. }
            | KindWithContent::IndexDeletion { .. }
            | KindWithContent::IndexCreation { .. }
            | KindWithContent::IndexUpdate { .. }
            | KindWithContent::IndexSwap { .. }
            | KindWithContent::TaskCancelation { .. }
            | KindWithContent::TaskDeletion { .. }
            | KindWithContent::DumpCreation { .. }
            | KindWithContent::SnapshotCreation => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KindWithContent {
    DocumentAdditionOrUpdate {
        index_uid: String,
        primary_key: Option<String>,
        method: IndexDocumentsMethod,
        content_file: Uuid,
        documents_count: u64,
        allow_index_creation: bool,
    },
    DocumentDeletion {
        index_uid: String,
        documents_ids: Vec<String>,
    },
    DocumentClear {
        index_uid: String,
    },
    SettingsUpdate {
        index_uid: String,
        new_settings: Box<Settings<Unchecked>>,
        is_deletion: bool,
        allow_index_creation: bool,
    },
    IndexDeletion {
        index_uid: String,
    },
    IndexCreation {
        index_uid: String,
        primary_key: Option<String>,
    },
    IndexUpdate {
        index_uid: String,
        primary_key: Option<String>,
    },
    IndexSwap {
        swaps: Vec<IndexSwap>,
    },
    TaskCancelation {
        query: String,
        tasks: RoaringBitmap,
    },
    TaskDeletion {
        query: String,
        tasks: RoaringBitmap,
    },
    DumpCreation {
        dump_uid: String,
        keys: Vec<Key>,
        instance_uid: Option<InstanceUid>,
    },
    SnapshotCreation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSwap {
    pub indexes: (String, String),
}

impl KindWithContent {
    pub fn as_kind(&self) -> Kind {
        match self {
            KindWithContent::DocumentAdditionOrUpdate { .. } => Kind::DocumentAdditionOrUpdate,
            KindWithContent::DocumentDeletion { .. } => Kind::DocumentDeletion,
            KindWithContent::DocumentClear { .. } => Kind::DocumentDeletion,
            KindWithContent::SettingsUpdate { .. } => Kind::SettingsUpdate,
            KindWithContent::IndexCreation { .. } => Kind::IndexCreation,
            KindWithContent::IndexDeletion { .. } => Kind::IndexDeletion,
            KindWithContent::IndexUpdate { .. } => Kind::IndexUpdate,
            KindWithContent::IndexSwap { .. } => Kind::IndexSwap,
            KindWithContent::TaskCancelation { .. } => Kind::TaskCancelation,
            KindWithContent::TaskDeletion { .. } => Kind::TaskDeletion,
            KindWithContent::DumpCreation { .. } => Kind::DumpCreation,
            KindWithContent::SnapshotCreation => Kind::SnapshotCreation,
        }
    }

    pub fn indexes(&self) -> Vec<&str> {
        use KindWithContent::*;

        match self {
            DumpCreation { .. }
            | SnapshotCreation
            | TaskCancelation { .. }
            | TaskDeletion { .. } => vec![],
            DocumentAdditionOrUpdate { index_uid, .. }
            | DocumentDeletion { index_uid, .. }
            | DocumentClear { index_uid }
            | SettingsUpdate { index_uid, .. }
            | IndexCreation { index_uid, .. }
            | IndexUpdate { index_uid, .. }
            | IndexDeletion { index_uid } => vec![index_uid],
            IndexSwap { swaps } => {
                let mut indexes = HashSet::<&str>::default();
                for swap in swaps {
                    indexes.insert(swap.indexes.0.as_str());
                    indexes.insert(swap.indexes.1.as_str());
                }
                indexes.into_iter().collect()
            }
        }
    }

    /// Returns the default `Details` that correspond to this `KindWithContent`,
    /// `None` if it cannot be generated.
    pub fn default_details(&self) -> Option<Details> {
        match self {
            KindWithContent::DocumentAdditionOrUpdate { documents_count, .. } => {
                Some(Details::DocumentAdditionOrUpdate {
                    received_documents: *documents_count,
                    indexed_documents: None,
                })
            }
            KindWithContent::DocumentDeletion { index_uid: _, documents_ids } => {
                Some(Details::DocumentDeletion {
                    matched_documents: documents_ids.len(),
                    deleted_documents: None,
                })
            }
            KindWithContent::DocumentClear { .. } => {
                Some(Details::ClearAll { deleted_documents: None })
            }
            KindWithContent::SettingsUpdate { new_settings, .. } => {
                Some(Details::SettingsUpdate { settings: new_settings.clone() })
            }
            KindWithContent::IndexDeletion { .. } => None,
            KindWithContent::IndexCreation { primary_key, .. }
            | KindWithContent::IndexUpdate { primary_key, .. } => {
                Some(Details::IndexInfo { primary_key: primary_key.clone() })
            }
            KindWithContent::IndexSwap { swaps } => {
                Some(Details::IndexSwap { swaps: swaps.clone() })
            }
            KindWithContent::TaskCancelation { query, tasks } => Some(Details::TaskCancelation {
                matched_tasks: tasks.len(),
                canceled_tasks: None,
                original_query: query.clone(),
            }),
            KindWithContent::TaskDeletion { query, tasks } => Some(Details::TaskDeletion {
                matched_tasks: tasks.len(),
                deleted_tasks: None,
                original_query: query.clone(),
            }),
            KindWithContent::DumpCreation { .. } => None,
            KindWithContent::SnapshotCreation => None,
        }
    }

    pub fn default_finished_details(&self) -> Option<Details> {
        match self {
            KindWithContent::DocumentAdditionOrUpdate { documents_count, .. } => {
                Some(Details::DocumentAdditionOrUpdate {
                    received_documents: *documents_count,
                    indexed_documents: Some(0),
                })
            }
            KindWithContent::DocumentDeletion { index_uid: _, documents_ids } => {
                Some(Details::DocumentDeletion {
                    matched_documents: documents_ids.len(),
                    deleted_documents: Some(0),
                })
            }
            KindWithContent::DocumentClear { .. } => {
                Some(Details::ClearAll { deleted_documents: None })
            }
            KindWithContent::SettingsUpdate { new_settings, .. } => {
                Some(Details::SettingsUpdate { settings: new_settings.clone() })
            }
            KindWithContent::IndexDeletion { .. } => None,
            KindWithContent::IndexCreation { primary_key, .. }
            | KindWithContent::IndexUpdate { primary_key, .. } => {
                Some(Details::IndexInfo { primary_key: primary_key.clone() })
            }
            KindWithContent::IndexSwap { .. } => {
                todo!()
            }
            KindWithContent::TaskCancelation { query, tasks } => Some(Details::TaskCancelation {
                matched_tasks: tasks.len(),
                canceled_tasks: Some(0),
                original_query: query.clone(),
            }),
            KindWithContent::TaskDeletion { query, tasks } => Some(Details::TaskDeletion {
                matched_tasks: tasks.len(),
                deleted_tasks: Some(0),
                original_query: query.clone(),
            }),
            KindWithContent::DumpCreation { .. } => None,
            KindWithContent::SnapshotCreation => None,
        }
    }
}

impl From<&KindWithContent> for Option<Details> {
    fn from(kind: &KindWithContent) -> Self {
        match kind {
            KindWithContent::DocumentAdditionOrUpdate { documents_count, .. } => {
                Some(Details::DocumentAdditionOrUpdate {
                    received_documents: *documents_count,
                    indexed_documents: None,
                })
            }
            KindWithContent::DocumentDeletion { .. } => None,
            KindWithContent::DocumentClear { .. } => None,
            KindWithContent::SettingsUpdate { new_settings, .. } => {
                Some(Details::SettingsUpdate { settings: new_settings.clone() })
            }
            KindWithContent::IndexDeletion { .. } => None,
            KindWithContent::IndexCreation { primary_key, .. } => {
                Some(Details::IndexInfo { primary_key: primary_key.clone() })
            }
            KindWithContent::IndexUpdate { primary_key, .. } => {
                Some(Details::IndexInfo { primary_key: primary_key.clone() })
            }
            KindWithContent::IndexSwap { .. } => None,
            KindWithContent::TaskCancelation { query, tasks } => Some(Details::TaskCancelation {
                matched_tasks: tasks.len(),
                canceled_tasks: None,
                original_query: query.clone(),
            }),
            KindWithContent::TaskDeletion { query, tasks } => Some(Details::TaskDeletion {
                matched_tasks: tasks.len(),
                deleted_tasks: None,
                original_query: query.clone(),
            }),
            KindWithContent::DumpCreation { dump_uid, .. } => {
                Some(Details::Dump { dump_uid: dump_uid.clone() })
            }
            KindWithContent::SnapshotCreation => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Sequence)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Enqueued,
    Processing,
    Succeeded,
    Failed,
    Canceled,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Enqueued => write!(f, "enqueued"),
            Status::Processing => write!(f, "processing"),
            Status::Succeeded => write!(f, "succeeded"),
            Status::Failed => write!(f, "failed"),
            Status::Canceled => write!(f, "canceled"),
        }
    }
}

impl FromStr for Status {
    type Err = ResponseError;

    fn from_str(status: &str) -> Result<Self, Self::Err> {
        if status.eq_ignore_ascii_case("enqueued") {
            Ok(Status::Enqueued)
        } else if status.eq_ignore_ascii_case("processing") {
            Ok(Status::Processing)
        } else if status.eq_ignore_ascii_case("succeeded") {
            Ok(Status::Succeeded)
        } else if status.eq_ignore_ascii_case("failed") {
            Ok(Status::Failed)
        } else if status.eq_ignore_ascii_case("canceled") {
            Ok(Status::Canceled)
        } else {
            Err(ResponseError::from_msg(
                format!(
                    "`{}` is not a status. Available status are {}.",
                    status,
                    enum_iterator::all::<Status>()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<String>>()
                        .join(", ")
                ),
                Code::BadRequest,
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Sequence)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    DocumentAdditionOrUpdate,
    DocumentDeletion,
    SettingsUpdate,
    IndexCreation,
    IndexDeletion,
    IndexUpdate,
    IndexSwap,
    TaskCancelation,
    TaskDeletion,
    DumpCreation,
    SnapshotCreation,
}

impl Kind {
    pub fn related_to_one_index(&self) -> bool {
        match self {
            Kind::DocumentAdditionOrUpdate
            | Kind::DocumentDeletion
            | Kind::SettingsUpdate
            | Kind::IndexCreation
            | Kind::IndexDeletion
            | Kind::IndexUpdate => true,
            Kind::IndexSwap
            | Kind::TaskCancelation
            | Kind::TaskDeletion
            | Kind::DumpCreation
            | Kind::SnapshotCreation => false,
        }
    }
}

impl FromStr for Kind {
    type Err = ResponseError;

    fn from_str(kind: &str) -> Result<Self, Self::Err> {
        if kind.eq_ignore_ascii_case("indexCreation") {
            Ok(Kind::IndexCreation)
        } else if kind.eq_ignore_ascii_case("indexUpdate") {
            Ok(Kind::IndexUpdate)
        } else if kind.eq_ignore_ascii_case("indexSwap") {
            Ok(Kind::IndexSwap)
        } else if kind.eq_ignore_ascii_case("indexDeletion") {
            Ok(Kind::IndexDeletion)
        } else if kind.eq_ignore_ascii_case("documentAdditionOrUpdate") {
            Ok(Kind::DocumentAdditionOrUpdate)
        } else if kind.eq_ignore_ascii_case("documentDeletion") {
            Ok(Kind::DocumentDeletion)
        } else if kind.eq_ignore_ascii_case("settingsUpdate") {
            Ok(Kind::SettingsUpdate)
        } else if kind.eq_ignore_ascii_case("taskCancelation") {
            Ok(Kind::TaskCancelation)
        } else if kind.eq_ignore_ascii_case("taskDeletion") {
            Ok(Kind::TaskDeletion)
        } else if kind.eq_ignore_ascii_case("dumpCreation") {
            Ok(Kind::DumpCreation)
        } else if kind.eq_ignore_ascii_case("snapshotCreation") {
            Ok(Kind::SnapshotCreation)
        } else {
            Err(ResponseError::from_msg(
                format!(
                    "`{}` is not a type. Available types are {}.",
                    kind,
                    enum_iterator::all::<Kind>()
                        .map(|k| format!(
                            "`{}`",
                            // by default serde is going to insert `"` around the value.
                            serde_json::to_string(&k).unwrap().trim_matches('"')
                        ))
                        .collect::<Vec<String>>()
                        .join(", ")
                ),
                Code::BadRequest,
            ))
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum Details {
    DocumentAdditionOrUpdate { received_documents: u64, indexed_documents: Option<u64> },
    SettingsUpdate { settings: Box<Settings<Unchecked>> },
    IndexInfo { primary_key: Option<String> },
    DocumentDeletion { matched_documents: usize, deleted_documents: Option<u64> },
    ClearAll { deleted_documents: Option<u64> },
    TaskCancelation { matched_tasks: u64, canceled_tasks: Option<u64>, original_query: String },
    TaskDeletion { matched_tasks: u64, deleted_tasks: Option<u64>, original_query: String },
    Dump { dump_uid: String },
    IndexSwap { swaps: Vec<IndexSwap> },
}

/// Serialize a `time::Duration` as a best effort ISO 8601 while waiting for
/// https://github.com/time-rs/time/issues/378.
/// This code is a port of the old code of time that was removed in 0.2.
pub fn serialize_duration<S: Serializer>(
    duration: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match duration {
        Some(duration) => {
            // technically speaking, negative duration is not valid ISO 8601
            if duration.is_negative() {
                return serializer.serialize_none();
            }

            const SECS_PER_DAY: i64 = Duration::DAY.whole_seconds();
            let secs = duration.whole_seconds();
            let days = secs / SECS_PER_DAY;
            let secs = secs - days * SECS_PER_DAY;
            let hasdate = days != 0;
            let nanos = duration.subsec_nanoseconds();
            let hastime = (secs != 0 || nanos != 0) || !hasdate;

            // all the following unwrap can't fail
            let mut res = String::new();
            write!(&mut res, "P").unwrap();

            if hasdate {
                write!(&mut res, "{}D", days).unwrap();
            }

            const NANOS_PER_MILLI: i32 = Duration::MILLISECOND.subsec_nanoseconds();
            const NANOS_PER_MICRO: i32 = Duration::MICROSECOND.subsec_nanoseconds();

            if hastime {
                if nanos == 0 {
                    write!(&mut res, "T{}S", secs).unwrap();
                } else if nanos % NANOS_PER_MILLI == 0 {
                    write!(&mut res, "T{}.{:03}S", secs, nanos / NANOS_PER_MILLI).unwrap();
                } else if nanos % NANOS_PER_MICRO == 0 {
                    write!(&mut res, "T{}.{:06}S", secs, nanos / NANOS_PER_MICRO).unwrap();
                } else {
                    write!(&mut res, "T{}.{:09}S", secs, nanos).unwrap();
                }
            }

            serializer.serialize_str(&res)
        }
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::Details;
    use crate::heed::types::SerdeJson;
    use crate::heed::{BytesDecode, BytesEncode};

    #[test]
    fn bad_deser() {
        let details = Details::TaskDeletion {
            matched_tasks: 1,
            deleted_tasks: None,
            original_query: "hello".to_owned(),
        };
        let serialised = SerdeJson::<Details>::bytes_encode(&details).unwrap();
        let deserialised = SerdeJson::<Details>::bytes_decode(&serialised).unwrap();
        meili_snap::snapshot!(format!("{:?}", details), @r###"TaskDeletion { matched_tasks: 1, deleted_tasks: None, original_query: "hello" }"###);
        meili_snap::snapshot!(format!("{:?}", deserialised), @r###"TaskDeletion { matched_tasks: 1, deleted_tasks: None, original_query: "hello" }"###);
    }
}
