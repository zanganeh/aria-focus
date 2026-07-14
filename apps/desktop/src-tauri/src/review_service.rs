use audio_engine::{decode_track, DecodeExpectation, DecodedProgram, MediaCodec, SourceLabel};
use serde::Serialize;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReviewCandidate {
    pub alias: String,
    pub title: String,
    pub review_id: String,
    pub bytes: u64,
    pub codec: String,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub duration_seconds: u32,
    pub quarantine_status: String,
}

struct Pin {
    alias: &'static str,
    file: &'static str,
    hash: &'static str,
    bytes: u64,
}

struct ReviewCatalogue {
    directory: &'static str,
    pack_id: &'static str,
    pack_title: &'static str,
    pins: &'static [Pin],
}

const DEEP_WORK_PINS: &[Pin] = &[
    Pin {
        alias: "E",
        file: "deep-work-still-cloud-070.flac",
        hash: "945c74c1f7aed0ce7858d1de6ab241c7f48e34b0e58fdfd31cdf6d97950a727d",
        bytes: 6977340,
    },
    Pin {
        alias: "F",
        file: "deep-work-still-ember-072.flac",
        hash: "2e263cb45dbcbf31647f5a3e955e471b11d6c730e95ef76ff1623d2559e101e9",
        bytes: 6556767,
    },
    Pin {
        alias: "G",
        file: "deep-work-still-dusk-068.flac",
        hash: "2c94f4dd431f177257764f3a0b2bd8b7465d9b4856da14cd54417ab0731d2ce1",
        bytes: 6786809,
    },
    Pin {
        alias: "H",
        file: "deep-work-still-tide-074.flac",
        hash: "5e82108abccb8853dd92c073c79f439744dc75eea2785081447ec626b92481bc",
        bytes: 6916187,
    },
];

const LEARNING_PINS: &[Pin] = &[
    Pin {
        alias: "I",
        file: "learning-clearfield-ambient-068.flac",
        hash: "96b05e7f1d29c0c23a4c48d3cd28cf43044deaaad5314c98907546ff08b5fa25",
        bytes: 6_760_623,
    },
    Pin {
        alias: "J",
        file: "learning-mossair-organic-072.flac",
        hash: "acaca32844e6e33ce78e32178ea235ccc2b389f59ee3b2ce04b94f42b4b09048",
        bytes: 7_187_104,
    },
    Pin {
        alias: "K",
        file: "learning-paperlight-classical-064.flac",
        hash: "4aadaf220e007f8d28e503f25a10fbb90817586a273f962ef02736fdbbe17ed9",
        bytes: 6_955_029,
    },
    Pin {
        alias: "L",
        file: "learning-softgrain-lofi-076.flac",
        hash: "afe7fbd0ad6d853e619d3b16a5207f4e59ea345d5ea88aec2a8ba9f1ddd8763d",
        bytes: 7_346_575,
    },
];

// These internal pins intentionally pair opaque labels with exact local bytes.
// They are never serialized, imported as a pack, or passed to feedback/publish
// validation. Only `alias` crosses the review command boundary.
const ACTIVITY_PINS: &[Pin] = &[
    Pin {
        alias: "M",
        file: "creativity-softmotion-downtempo-086.flac",
        hash: "75fdcc6b23b967fcf82a10f45ea423af884a9da0b5c0529fdfcff40ea2ce63c2",
        bytes: 6_795_987,
    },
    Pin {
        alias: "N",
        file: "creativity-threadlight-classical-068.flac",
        hash: "2e41ceaf7c7970385fb66bcc8fdf3f011da42758210f64ac0b5c42726037d9e3",
        bytes: 7_133_308,
    },
    Pin {
        alias: "O",
        file: "creativity-prismfield-ambient-078.flac",
        hash: "fd9dc24b17d15407197a1dfdccb8020f7cfcbe35c06ebb16e5078898893d57e0",
        bytes: 6_677_526,
    },
    Pin {
        alias: "P",
        file: "creativity-inkroom-jazz-074.flac",
        hash: "36050ea9d3357503949587cf6115c4df84c96564c68dc1ad056a729e9a009687",
        bytes: 6_986_845,
    },
    Pin {
        alias: "Q",
        file: "motivation-groundbeat-hiphop-092.flac",
        hash: "d0a72851e5bc94272962aaaaee9d9ccace0271caa033dabe594e5b3ede25c765",
        bytes: 6_703_878,
    },
    Pin {
        alias: "R",
        file: "motivation-riseplain-orchestral-088.flac",
        hash: "0d529d4190cc10ff85806233a0d3cc00d20b41130e1157574c35e27f5281bcbf",
        bytes: 7_664_752,
    },
    Pin {
        alias: "S",
        file: "motivation-forwardgrid-electronic-104.flac",
        hash: "bcae720112549ab9f39bb42c5d4ab976fbc362b9ab78bcfb7f363b1bea8bb5ef",
        bytes: 6_745_563,
    },
    Pin {
        alias: "T",
        file: "motivation-neonsteady-synthwave-096.flac",
        hash: "bcf0c7ca1795c16d350a0724340d2ed903c7c40e95881a59ef9273b0378b510b",
        bytes: 7_078_482,
    },
    Pin {
        alias: "U",
        file: "lightwork-sunpaper-acoustic-082.flac",
        hash: "88252f9b24616503ee56f48b0e10e769f8c48b4f4e4e80132bc65ace7aa7c301",
        bytes: 7_260_313,
    },
    Pin {
        alias: "V",
        file: "lightwork-glassair-electronic-086.flac",
        hash: "f0a5390a6ad40d1d7b4293695ddddb4ff9fef5533028d4d035f70aa58234b9f0",
        bytes: 6_816_697,
    },
    Pin {
        alias: "W",
        file: "lightwork-windowtable-jazz-078.flac",
        hash: "6c7d7a3bce1b17c5938652890b3fdd94ab229dcf8cf5dbcdac80a0f7182bb9ed",
        bytes: 7_772_910,
    },
    Pin {
        alias: "X",
        file: "lightwork-easystep-downtempo-090.flac",
        hash: "9bd98ead130db86a83e370226e2df082d46f81a77a984698acb9236eff9d1749",
        bytes: 6_857_619,
    },
];

const DEEP_WORK_CATALOGUE: ReviewCatalogue = ReviewCatalogue {
    directory: "review-candidates",
    pack_id: "quarantined-review",
    pack_title: "Quarantined local review — not approved/published",
    pins: DEEP_WORK_PINS,
};

const LEARNING_CATALOGUE: ReviewCatalogue = ReviewCatalogue {
    directory: "learning-review-candidates",
    pack_id: "quarantined-learning-review",
    pack_title: "Quarantined Learning local review — not approved/published",
    pins: LEARNING_PINS,
};

const ACTIVITY_CATALOGUE: ReviewCatalogue = ReviewCatalogue {
    directory: "activity-review-candidates",
    pack_id: "quarantined-activity-review",
    pack_title: "Quarantined Activity Review — not approved/published",
    pins: ACTIVITY_PINS,
};

pub(crate) struct ReviewService {
    root: PathBuf,
    catalogue: &'static ReviewCatalogue,
}

impl ReviewService {
    pub(crate) fn new(resource: PathBuf) -> Self {
        let catalogue = if resource.join(DEEP_WORK_CATALOGUE.directory).is_dir() {
            &DEEP_WORK_CATALOGUE
        } else if resource.join(LEARNING_CATALOGUE.directory).is_dir() {
            &LEARNING_CATALOGUE
        } else {
            &ACTIVITY_CATALOGUE
        };
        Self {
            root: resource.join(catalogue.directory),
            catalogue,
        }
    }

    /// A candidate is visible only after the same bounded decode, exact bytes,
    /// hash, codec, and metadata checks used immediately before playback.
    pub(crate) fn list(&self) -> Vec<ReviewCandidate> {
        self.catalogue
            .pins
            .iter()
            .filter(|pin| self.prepare_pin(pin).is_ok())
            .map(meta)
            .collect()
    }

    pub(crate) fn available(&self) -> bool {
        !self.list().is_empty()
    }

    pub(crate) fn prepare(&self, review_id: &str) -> Result<DecodedProgram, String> {
        let pin = self
            .catalogue
            .pins
            .iter()
            // The frontend receives only this opaque blind label. Candidate
            // identities remain in the local, compiled pin catalogue rather
            // than bundled review metadata.
            .find(|pin| pin.alias == review_id)
            .ok_or("Unknown quarantined review candidate.")?;
        self.prepare_pin(pin)
    }

    fn prepare_pin(&self, pin: &Pin) -> Result<DecodedProgram, String> {
        let track = decode_track(&DecodeExpectation {
            path: self.path(pin)?,
            codec: MediaCodec::Flac,
            bytes: pin.bytes,
            sha256: pin.hash.into(),
            sample_rate_hz: 48_000,
            channels: 2,
            bit_depth: Some(16),
            duration_seconds: 90.0,
            // Deliberately empty: these candidates have no authored safe loop
            // region. PlaybackSource::Review applies a visible provisional
            // boundary crossfade solely for local evaluation.
            regions: vec![],
            label: opaque_source_label(self.catalogue, pin),
        })
        .map_err(|error| format!("Quarantined review resource rejected: {error}"))?;
        DecodedProgram::new(vec![track]).map_err(|error| error.to_string())
    }

    fn path(&self, pin: &Pin) -> Result<PathBuf, String> {
        let root_metadata = std::fs::symlink_metadata(&self.root)
            .map_err(|_| "Quarantined review resources are not staged.".to_owned())?;
        if link_or_reparse_point(&root_metadata) || !root_metadata.is_dir() {
            return Err("Quarantined review resource root link/substitution rejected.".into());
        }
        let root = self
            .root
            .canonicalize()
            .map_err(|_| "Quarantined review resources are not staged.".to_owned())?;
        let raw = self.root.join(pin.file);
        let metadata = std::fs::symlink_metadata(&raw)
            .map_err(|_| "Quarantined review resource is missing.".to_owned())?;
        if link_or_reparse_point(&metadata) || !metadata.is_file() {
            return Err("Quarantined review resource link/substitution rejected.".into());
        }
        let resolved = raw
            .canonicalize()
            .map_err(|_| "Quarantined review resource cannot be resolved.".to_owned())?;
        if !resolved.starts_with(&root)
            || resolved.parent() != Some(root.as_path())
            || Path::new(pin.file).file_name().is_none()
        {
            return Err("Quarantined review resource escaped its resource directory.".into());
        }
        Ok(resolved)
    }
}

fn link_or_reparse_point(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn meta(pin: &Pin) -> ReviewCandidate {
    ReviewCandidate {
        alias: pin.alias.into(),
        title: format!("Track {}", pin.alias),
        review_id: pin.alias.into(),
        bytes: pin.bytes,
        codec: "FLAC".into(),
        sample_rate_hz: 48_000,
        channels: 2,
        duration_seconds: 90,
        quarantine_status: "local_evaluation_only_not_approved_or_published_provisional_transition"
            .into(),
    }
}

fn opaque_source_label(catalogue: &ReviewCatalogue, pin: &Pin) -> SourceLabel {
    SourceLabel {
        pack_id: catalogue.pack_id.into(),
        pack_title: catalogue.pack_title.into(),
        item_id: format!("blind-{}", pin.alias.to_ascii_lowercase()),
        item_title: format!("Track {} — quarantined review", pin.alias),
        variant_id: "opaque-review-source".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_file_is_not_a_link_or_reparse_point() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("ordinary.flac");
        std::fs::write(&file, [0_u8; 1]).unwrap();
        assert!(!link_or_reparse_point(
            &std::fs::symlink_metadata(file).unwrap()
        ));
    }

    #[cfg(unix)]
    #[test]
    fn portable_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("review-candidates");
        std::fs::create_dir(&root).unwrap();
        let target = directory.path().join("outside.flac");
        std::fs::write(&target, [0_u8; 1]).unwrap();
        symlink(&target, root.join(DEEP_WORK_PINS[0].file)).unwrap();
        assert!(ReviewService {
            root,
            catalogue: &DEEP_WORK_CATALOGUE,
        }
        .path(&DEEP_WORK_PINS[0])
        .is_err());
    }

    #[test]
    fn learning_catalogue_keeps_identity_out_of_review_metadata() {
        let metadata = meta(&LEARNING_PINS[0]);
        let serialized = serde_json::to_string(&metadata).unwrap();
        let label = opaque_source_label(&LEARNING_CATALOGUE, &LEARNING_PINS[0]);
        let internal_identity = LEARNING_PINS[0].file.trim_end_matches(".flac");

        assert_eq!(metadata.alias, "I");
        assert_eq!(metadata.review_id, "I");
        assert!(!metadata.title.contains(internal_identity));
        assert!(!metadata.title.contains("ambient"));
        assert!(!serialized.contains(internal_identity));
        assert!(!serialized.contains(LEARNING_PINS[0].hash));
        assert_eq!(label.item_id, "blind-i");
        assert_eq!(label.variant_id, "opaque-review-source");
        assert!(!format!("{label:?}").contains(internal_identity));
        assert!(!format!("{label:?}").contains(LEARNING_PINS[0].hash));
    }

    #[test]
    fn review_catalogues_have_disjoint_blind_labels_and_resource_directories() {
        assert_ne!(DEEP_WORK_CATALOGUE.directory, LEARNING_CATALOGUE.directory);
        assert_ne!(DEEP_WORK_CATALOGUE.directory, ACTIVITY_CATALOGUE.directory);
        assert_ne!(LEARNING_CATALOGUE.directory, ACTIVITY_CATALOGUE.directory);
        assert!(DEEP_WORK_CATALOGUE
            .pins
            .iter()
            .all(|deep| LEARNING_CATALOGUE
                .pins
                .iter()
                .all(|learning| deep.alias != learning.alias)));
        assert!(ACTIVITY_CATALOGUE.pins.iter().all(|activity| {
            DEEP_WORK_CATALOGUE
                .pins
                .iter()
                .chain(LEARNING_CATALOGUE.pins)
                .all(|existing| existing.alias != activity.alias)
        }));
    }

    #[test]
    fn staged_learning_catalogue_decodes_all_sources_with_opaque_labels() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(LEARNING_CATALOGUE.directory);
        if !root.is_dir() {
            // Hosted and ordinary builds intentionally contain no review audio.
            return;
        }
        let service = ReviewService {
            root,
            catalogue: &LEARNING_CATALOGUE,
        };
        assert_eq!(service.list().len(), LEARNING_PINS.len());
        for pin in LEARNING_PINS {
            let program = service.prepare(pin.alias).unwrap();
            assert_eq!(program.tracks.len(), 1);
            let label = &program.tracks[0].label;
            assert_eq!(
                label.item_id,
                format!("blind-{}", pin.alias.to_ascii_lowercase())
            );
            assert_eq!(label.variant_id, "opaque-review-source");
            assert!(!format!("{label:?}").contains(pin.file.trim_end_matches(".flac")));
            assert!(!format!("{label:?}").contains(pin.hash));
        }
    }

    #[test]
    fn activity_catalogue_serialization_and_source_metadata_are_blind() {
        for pin in ACTIVITY_PINS {
            let metadata = meta(pin);
            let serialized = serde_json::to_string(&metadata).unwrap();
            let label = opaque_source_label(&ACTIVITY_CATALOGUE, pin);
            let internal_identity = pin.file.trim_end_matches(".flac");

            assert_eq!(metadata.alias, pin.alias);
            assert_eq!(metadata.review_id, pin.alias);
            assert!(metadata.title.ends_with(pin.alias));
            for forbidden in [
                internal_identity,
                pin.hash,
                "creativity",
                "motivation",
                "lightwork",
            ] {
                assert!(!serialized.contains(forbidden));
                assert!(!format!("{label:?}").contains(forbidden));
            }
            assert_eq!(
                label.item_id,
                format!("blind-{}", pin.alias.to_ascii_lowercase())
            );
            assert_eq!(label.variant_id, "opaque-review-source");
        }
    }

    #[test]
    fn staged_activity_catalogue_decodes_all_sources_with_opaque_labels() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(ACTIVITY_CATALOGUE.directory);
        if !root.is_dir() {
            // Standard, Deep Work, and Learning builds intentionally omit these bytes.
            return;
        }
        let service = ReviewService {
            root,
            catalogue: &ACTIVITY_CATALOGUE,
        };
        assert_eq!(service.list().len(), ACTIVITY_PINS.len());
        for pin in ACTIVITY_PINS {
            let program = service.prepare(pin.alias).unwrap();
            assert_eq!(program.tracks.len(), 1);
            let label = &program.tracks[0].label;
            assert_eq!(
                label.item_id,
                format!("blind-{}", pin.alias.to_ascii_lowercase())
            );
            assert_eq!(label.variant_id, "opaque-review-source");
            let debug = format!("{label:?}");
            assert!(!debug.contains(pin.file.trim_end_matches(".flac")));
            assert!(!debug.contains(pin.hash));
        }
    }
}
