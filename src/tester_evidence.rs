use crate::{
    execution_graph::{PrerequisiteState, TestCheckpointSpec},
    plan::EvidenceMode,
    tester_workspace::TesterArtifactRef,
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TesterClassification {
    ProductFailure,
    TestFailure,
    EnvironmentFailure,
    NotReady,
    IntegrationNotReady,
    SpecGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceProvenance {
    Observed,
    Implication,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TesterModeOutcome {
    Pass,
    Fail,
    Complete,
    Blocked,
    NeedsHuman,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterModeResult {
    pub mode: EvidenceMode,
    pub outcome: TesterModeOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl TesterModeResult {
    pub fn validate(&self) -> Result<()> {
        let valid = match self.mode {
            EvidenceMode::Verify => matches!(
                self.outcome,
                TesterModeOutcome::Pass
                    | TesterModeOutcome::Fail
                    | TesterModeOutcome::Blocked
                    | TesterModeOutcome::NeedsHuman
            ),
            EvidenceMode::Measure | EvidenceMode::Probe => matches!(
                self.outcome,
                TesterModeOutcome::Complete
                    | TesterModeOutcome::Blocked
                    | TesterModeOutcome::NeedsHuman
            ),
        };
        if !valid {
            bail!(
                "Tester outcome {:?} is invalid for mode {:?}",
                self.outcome,
                self.mode
            );
        }
        if matches!(
            self.outcome,
            TesterModeOutcome::Fail
                | TesterModeOutcome::Blocked
                | TesterModeOutcome::NeedsHuman
        ) && self
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            bail!("non-success Tester mode outcome requires a reason");
        }
        Ok(())
    }

    pub fn satisfies_output_requirement(&self) -> bool {
        matches!(
            (self.mode, self.outcome),
            (EvidenceMode::Verify, TesterModeOutcome::Pass)
                | (
                    EvidenceMode::Measure | EvidenceMode::Probe,
                    TesterModeOutcome::Complete
                )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservedValue {
    Text(String),
    Integer(i64),
    Decimal(String),
    Boolean(bool),
}

impl ObservedValue {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Text(value) if value.trim().is_empty() => {
                bail!("observed text value cannot be empty")
            }
            Self::Decimal(value) => {
                let parsed = value
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("observed decimal value is not numeric"))?;
                if !parsed.is_finite() {
                    bail!("observed decimal value must be finite");
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TesterEvidenceRef {
    WorkspaceArtifact { artifact: TesterArtifactRef },
    VerificationRun { run_id: i64 },
    AdapterObservation {
        adapter_id: String,
        execution_id: String,
    },
}

impl TesterEvidenceRef {
    fn validate(&self) -> Result<()> {
        match self {
            Self::WorkspaceArtifact { artifact } => {
                require_text("workspace artifact path", &artifact.path)?;
                validate_sha256(&artifact.sha256)?;
            }
            Self::VerificationRun { run_id } => {
                if *run_id <= 0 {
                    bail!("verification run evidence ref must be positive");
                }
            }
            Self::AdapterObservation {
                adapter_id,
                execution_id,
            } => {
                require_text("adapter observation adapter_id", adapter_id)?;
                require_text("adapter observation execution_id", execution_id)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentObservation {
    pub name: String,
    pub value: ObservedValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub evidence_refs: Vec<TesterEvidenceRef>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ExperimentObservation {
    fn validate(&self) -> Result<()> {
        require_text("experiment observation name", &self.name)?;
        self.value.validate()?;
        if let Some(unit) = &self.unit {
            require_text("experiment observation unit", unit)?;
        }
        if self.evidence_refs.is_empty() {
            bail!("experiment observation requires at least one evidence ref");
        }
        for evidence_ref in &self.evidence_refs {
            evidence_ref.validate()?;
        }
        validate_text_items("experiment observation limitation", &self.limitations)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentSample {
    pub sample_index: u32,
    pub variables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_set_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_fingerprint: Option<String>,
    pub observations: Vec<ExperimentObservation>,
}

impl ExperimentSample {
    fn validate(&self, dimensions: &[String]) -> Result<()> {
        if self.sample_index == 0 {
            bail!("experiment sample_index must start at 1");
        }
        for dimension in dimensions {
            let value = self
                .variables
                .get(dimension)
                .ok_or_else(|| anyhow::anyhow!("sample missing declared dimension {dimension}"))?;
            require_text("experiment variable value", value)?;
        }
        for key in self.variables.keys() {
            if !dimensions.iter().any(|dimension| dimension == key) {
                bail!("sample contains undeclared experiment dimension {key}");
            }
        }
        for value in [
            self.boundary_event.as_deref(),
            self.target_revision.as_deref(),
            self.change_set_id.as_deref(),
            self.runtime_identity.as_deref(),
            self.capability_fingerprint.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            require_text("experiment context value", value)?;
        }
        if self.observations.is_empty() {
            bail!("experiment sample requires at least one observation");
        }
        let mut names = HashSet::new();
        for observation in &self.observations {
            observation.validate()?;
            if !names.insert(observation.name.as_str()) {
                bail!(
                    "duplicate experiment observation {} in sample {}",
                    observation.name,
                    self.sample_index
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentContext {
    pub dimensions: Vec<String>,
    pub samples: Vec<ExperimentSample>,
}

impl ExperimentContext {
    pub fn validate(&self) -> Result<()> {
        let mut dimensions = HashSet::new();
        for dimension in &self.dimensions {
            require_text("experiment dimension", dimension)?;
            if !dimensions.insert(dimension.as_str()) {
                bail!("duplicate experiment dimension {dimension}");
            }
        }
        if self.samples.is_empty() {
            bail!("experiment context requires at least one sample");
        }
        let mut sample_indexes = HashSet::new();
        for sample in &self.samples {
            if !sample_indexes.insert(sample.sample_index) {
                bail!("duplicate experiment sample_index {}", sample.sample_index);
            }
            sample.validate(&self.dimensions)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicabilityDimension {
    ProductRevision,
    ChangeSetId,
    ConfigFingerprint,
    CapabilityFingerprint,
    EnvironmentFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "matcher", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicabilityMatcher {
    ExactValue {
        dimension: ApplicabilityDimension,
        value: String,
    },
    ExactFingerprint {
        dimension: ApplicabilityDimension,
        sha256: String,
    },
    MemberOf {
        dimension: ApplicabilityDimension,
        values: Vec<String>,
    },
    SourcePathHashSet {
        hashes: BTreeMap<String, String>,
    },
    RuntimeIdentity {
        value: String,
    },
}

impl ApplicabilityMatcher {
    fn validate(&self) -> Result<()> {
        match self {
            Self::ExactValue { value, .. } => require_text("EXACT_VALUE", value),
            Self::ExactFingerprint { sha256, .. } => validate_sha256(sha256),
            Self::MemberOf { values, .. } => {
                if values.is_empty() {
                    bail!("MEMBER_OF requires at least one value");
                }
                validate_text_items("MEMBER_OF value", values)?;
                let unique = values.iter().collect::<HashSet<_>>();
                if unique.len() != values.len() {
                    bail!("MEMBER_OF values must be unique");
                }
                Ok(())
            }
            Self::SourcePathHashSet { hashes } => {
                if hashes.is_empty() {
                    bail!("SOURCE_PATH_HASH_SET requires at least one path hash");
                }
                for (path, sha256) in hashes {
                    require_text("SOURCE_PATH_HASH_SET path", path)?;
                    if path.starts_with('/')
                        || path.contains('\\')
                        || path.contains(':')
                        || path
                            .split('/')
                            .any(|part| part == "..")
                    {
                        bail!("SOURCE_PATH_HASH_SET contains unsafe path {path}");
                    }
                    validate_sha256(sha256)?;
                }
                Ok(())
            }
            Self::RuntimeIdentity { value } => require_text("RUNTIME_IDENTITY", value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevalidationPolicy {
    ReuseIfMatches,
    AlwaysRevalidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceApplicability {
    pub policy: RevalidationPolicy,
    #[serde(default)]
    pub matchers: Vec<ApplicabilityMatcher>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApplicabilityContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_set_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_fingerprint: Option<String>,
    #[serde(default)]
    pub source_path_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicabilityDecision {
    Compatible,
    Invalidated,
    RevalidationRequired,
}

impl EvidenceApplicability {
    pub fn validate(&self) -> Result<()> {
        if self.policy == RevalidationPolicy::ReuseIfMatches && self.matchers.is_empty() {
            bail!("REUSE_IF_MATCHES applicability requires at least one matcher");
        }
        for matcher in &self.matchers {
            matcher.validate()?;
        }
        Ok(())
    }

    pub fn evaluate(&self, context: &ApplicabilityContext) -> Result<ApplicabilityDecision> {
        self.validate()?;
        if self.policy == RevalidationPolicy::AlwaysRevalidate {
            return Ok(ApplicabilityDecision::RevalidationRequired);
        }
        for matcher in &self.matchers {
            if !matcher_matches(matcher, context) {
                return Ok(ApplicabilityDecision::Invalidated);
            }
        }
        Ok(ApplicabilityDecision::Compatible)
    }
}

fn matcher_matches(matcher: &ApplicabilityMatcher, context: &ApplicabilityContext) -> bool {
    match matcher {
        ApplicabilityMatcher::ExactValue { dimension, value } => {
            scalar_dimension(context, *dimension) == Some(value.as_str())
        }
        ApplicabilityMatcher::ExactFingerprint { dimension, sha256 } => {
            scalar_dimension(context, *dimension) == Some(sha256.as_str())
        }
        ApplicabilityMatcher::MemberOf { dimension, values } => scalar_dimension(context, *dimension)
            .map(|current| values.iter().any(|value| value == current))
            .unwrap_or(false),
        ApplicabilityMatcher::SourcePathHashSet { hashes } => hashes.iter().all(|(path, expected)| {
            context
                .source_path_hashes
                .get(path)
                .map(|current| current == expected)
                .unwrap_or(false)
        }),
        ApplicabilityMatcher::RuntimeIdentity { value } => {
            context.runtime_identity.as_deref() == Some(value.as_str())
        }
    }
}

fn scalar_dimension(
    context: &ApplicabilityContext,
    dimension: ApplicabilityDimension,
) -> Option<&str> {
    match dimension {
        ApplicabilityDimension::ProductRevision => context.product_revision.as_deref(),
        ApplicabilityDimension::ChangeSetId => context.change_set_id.as_deref(),
        ApplicabilityDimension::ConfigFingerprint => context.config_fingerprint.as_deref(),
        ApplicabilityDimension::CapabilityFingerprint => {
            context.capability_fingerprint.as_deref()
        }
        ApplicabilityDimension::EnvironmentFingerprint => {
            context.environment_fingerprint.as_deref()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterPrerequisiteTarget {
    pub jobpack_id: String,
    pub state: PrerequisiteState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_set_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterTargetBinding {
    #[serde(default)]
    pub prerequisites: Vec<TesterPrerequisiteTarget>,
}

impl TesterTargetBinding {
    pub fn fingerprint(&self, graph_version: i64, checkpoint_id: &str) -> Result<String> {
        if graph_version <= 0 {
            bail!("target graph_version must be positive");
        }
        require_text("target checkpoint_id", checkpoint_id)?;
        self.validate()?;
        let mut prerequisites = self.prerequisites.clone();
        prerequisites.sort_by(|left, right| left.jobpack_id.cmp(&right.jobpack_id));
        let canonical = serde_json::to_vec(&(graph_version, checkpoint_id, prerequisites))?;
        Ok(hex_digest(Sha256::digest(canonical)))
    }

    pub fn validate(&self) -> Result<()> {
        let mut jobpacks = HashSet::new();
        for target in &self.prerequisites {
            require_text("target prerequisite jobpack_id", &target.jobpack_id)?;
            if !jobpacks.insert(target.jobpack_id.as_str()) {
                bail!("duplicate target prerequisite {}", target.jobpack_id);
            }
            if target.state == PrerequisiteState::ReviewPass
                && target
                    .change_set_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
            {
                bail!(
                    "REVIEW_PASS prerequisite {} requires change_set_id",
                    target.jobpack_id
                );
            }
            if let Some(value) = &target.change_set_id {
                require_text("target change_set_id", value)?;
            }
            if let Some(value) = &target.target_revision {
                require_text("target revision", value)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterEvidenceOutputRecord {
    pub output_id: String,
    pub mode: EvidenceMode,
    pub provenance: EvidenceProvenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ObservedValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<TesterEvidenceRef>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub applicability: EvidenceApplicability,
}

impl TesterEvidenceOutputRecord {
    pub fn validate(&self) -> Result<()> {
        require_text("evidence output_id", &self.output_id)?;
        if let Some(value) = &self.value {
            value.validate()?;
        }
        if let Some(unit) = &self.unit {
            require_text("evidence output unit", unit)?;
        }
        validate_text_items("evidence output limitation", &self.limitations)?;
        for evidence_ref in &self.evidence_refs {
            evidence_ref.validate()?;
        }
        match self.provenance {
            EvidenceProvenance::Observed => {
                if self.value.is_none() || self.evidence_refs.is_empty() {
                    bail!("OBSERVED evidence requires a value and evidence refs");
                }
            }
            EvidenceProvenance::Implication => {
                if self.evidence_refs.is_empty() {
                    bail!("IMPLICATION evidence requires supporting evidence refs");
                }
            }
            EvidenceProvenance::Unresolved => {
                if self.value.is_some() {
                    bail!("UNRESOLVED evidence must not contain a resolved value");
                }
            }
        }
        self.applicability.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterAttemptEvidence {
    pub graph_version: i64,
    pub checkpoint_id: String,
    pub attempt_id: String,
    pub target: TesterTargetBinding,
    pub mode_results: Vec<TesterModeResult>,
    #[serde(default)]
    pub classifications: Vec<TesterClassification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<ExperimentContext>,
    #[serde(default)]
    pub outputs: Vec<TesterEvidenceOutputRecord>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl TesterAttemptEvidence {
    pub fn validate_against_checkpoint(&self, checkpoint: &TestCheckpointSpec) -> Result<()> {
        if self.graph_version <= 0 {
            bail!("Tester attempt graph_version must be positive");
        }
        require_text("Tester attempt checkpoint_id", &self.checkpoint_id)?;
        require_text("Tester attempt attempt_id", &self.attempt_id)?;
        if self.checkpoint_id != checkpoint.id {
            bail!(
                "Tester attempt checkpoint {} does not match contract {}",
                self.checkpoint_id,
                checkpoint.id
            );
        }
        self.target.validate()?;
        validate_text_items("Tester attempt limitation", &self.limitations)?;

        let expected_prerequisites = checkpoint
            .prerequisites
            .iter()
            .map(|item| (item.jobpack_id.as_str(), item.state))
            .collect::<BTreeMap<_, _>>();
        let actual_prerequisites = self
            .target
            .prerequisites
            .iter()
            .map(|item| (item.jobpack_id.as_str(), item.state))
            .collect::<BTreeMap<_, _>>();
        if expected_prerequisites != actual_prerequisites {
            bail!("Tester target prerequisites do not match checkpoint contract");
        }

        let mut result_modes = HashSet::new();
        for result in &self.mode_results {
            result.validate()?;
            if !result_modes.insert(result.mode) {
                bail!("duplicate Tester mode result {:?}", result.mode);
            }
        }
        let expected_modes = checkpoint.modes.iter().copied().collect::<HashSet<_>>();
        if result_modes != expected_modes {
            bail!("Tester mode results must cover every declared checkpoint mode exactly once");
        }

        let needs_experiment = self.mode_results.iter().any(|result| {
            matches!(result.mode, EvidenceMode::Measure | EvidenceMode::Probe)
                && result.outcome == TesterModeOutcome::Complete
        });
        if needs_experiment && self.experiment.is_none() {
            bail!("completed MEASURE/PROBE requires ExperimentContext");
        }
        if let Some(experiment) = &self.experiment {
            experiment.validate()?;
            for dimension in &checkpoint.experiment_dimensions {
                if !experiment
                    .dimensions
                    .iter()
                    .any(|candidate| candidate == dimension)
                {
                    bail!("ExperimentContext is missing checkpoint dimension {dimension}");
                }
            }
        }

        let mut output_ids = HashSet::new();
        for output in &self.outputs {
            output.validate()?;
            if !output_ids.insert(output.output_id.as_str()) {
                bail!("duplicate Tester evidence output {}", output.output_id);
            }
            let contract = checkpoint
                .evidence_outputs
                .iter()
                .find(|candidate| candidate.id == output.output_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Tester evidence output {} is not declared by checkpoint",
                        output.output_id
                    )
                })?;
            if contract.mode != output.mode {
                bail!(
                    "Tester evidence output {} mode does not match checkpoint contract",
                    output.output_id
                );
            }
        }

        for contract in &checkpoint.evidence_outputs {
            let successful = self
                .mode_results
                .iter()
                .find(|result| result.mode == contract.mode)
                .map(TesterModeResult::satisfies_output_requirement)
                .unwrap_or(false);
            if contract.required && successful {
                let output = self
                    .outputs
                    .iter()
                    .find(|output| output.output_id == contract.id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "successful mode {:?} is missing required evidence output {}",
                            contract.mode,
                            contract.id
                        )
                    })?;
                if output.provenance != EvidenceProvenance::Observed {
                    bail!(
                        "required successful evidence output {} must have OBSERVED provenance",
                        contract.id
                    );
                }
            }
        }

        let any_non_success = self.mode_results.iter().any(|result| {
            !matches!(
                result.outcome,
                TesterModeOutcome::Pass | TesterModeOutcome::Complete
            )
        });
        if any_non_success && self.classifications.is_empty() {
            bail!("non-success Tester attempt requires at least one classification");
        }
        let unique_classifications = self.classifications.iter().collect::<HashSet<_>>();
        if unique_classifications.len() != self.classifications.len() {
            bail!("Tester classifications must be unique");
        }
        Ok(())
    }

    pub fn target_fingerprint(&self) -> Result<String> {
        self.target
            .fingerprint(self.graph_version, &self.checkpoint_id)
    }
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("fingerprint must be a 64-character hexadecimal SHA-256");
    }
    Ok(())
}

fn require_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(())
}

fn validate_text_items(name: &str, values: &[String]) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("{name} entries must not be empty");
    }
    Ok(())
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let bytes = digest.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(value: char) -> String {
        std::iter::repeat(value).take(64).collect()
    }

    #[test]
    fn applicability_is_runtime_evaluated() {
        let applicability = EvidenceApplicability {
            policy: RevalidationPolicy::ReuseIfMatches,
            matchers: vec![
                ApplicabilityMatcher::ExactFingerprint {
                    dimension: ApplicabilityDimension::ConfigFingerprint,
                    sha256: sha('a'),
                },
                ApplicabilityMatcher::RuntimeIdentity {
                    value: "runtime-1".into(),
                },
                ApplicabilityMatcher::SourcePathHashSet {
                    hashes: BTreeMap::from([("src/session.rs".into(), sha('b'))]),
                },
            ],
        };
        let mut context = ApplicabilityContext {
            config_fingerprint: Some(sha('a')),
            runtime_identity: Some("runtime-1".into()),
            source_path_hashes: BTreeMap::from([("src/session.rs".into(), sha('b'))]),
            ..Default::default()
        };
        assert_eq!(
            applicability.evaluate(&context).unwrap(),
            ApplicabilityDecision::Compatible
        );
        context.runtime_identity = Some("runtime-2".into());
        assert_eq!(
            applicability.evaluate(&context).unwrap(),
            ApplicabilityDecision::Invalidated
        );
    }

    #[test]
    fn always_revalidate_never_reuses_evidence() {
        let applicability = EvidenceApplicability {
            policy: RevalidationPolicy::AlwaysRevalidate,
            matchers: vec![],
        };
        assert_eq!(
            applicability
                .evaluate(&ApplicabilityContext::default())
                .unwrap(),
            ApplicabilityDecision::RevalidationRequired
        );
    }

    #[test]
    fn target_fingerprint_is_stable_across_prerequisite_order() {
        let left = TesterTargetBinding {
            prerequisites: vec![
                TesterPrerequisiteTarget {
                    jobpack_id: "JP-B".into(),
                    state: PrerequisiteState::Done,
                    change_set_id: None,
                    target_revision: Some("rev-b".into()),
                },
                TesterPrerequisiteTarget {
                    jobpack_id: "JP-A".into(),
                    state: PrerequisiteState::ReviewPass,
                    change_set_id: Some("change-a".into()),
                    target_revision: Some("rev-a".into()),
                },
            ],
        };
        let right = TesterTargetBinding {
            prerequisites: left.prerequisites.iter().cloned().rev().collect(),
        };
        assert_eq!(
            left.fingerprint(1, "CP1").unwrap(),
            right.fingerprint(1, "CP1").unwrap()
        );
    }

    #[test]
    fn mode_outcomes_do_not_invent_measurement_verdicts() {
        assert!(TesterModeResult {
            mode: EvidenceMode::Measure,
            outcome: TesterModeOutcome::Pass,
            reason: None,
        }
        .validate()
        .is_err());
        assert!(TesterModeResult {
            mode: EvidenceMode::Verify,
            outcome: TesterModeOutcome::Complete,
            reason: None,
        }
        .validate()
        .is_err());
    }
}
