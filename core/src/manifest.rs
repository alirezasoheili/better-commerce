use std::collections::{BTreeMap, BTreeSet};

use semver::{Version, VersionReq};
use serde::Deserialize;

const FOUNDATION_CONTRACT: &str = "foundation";

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    SelfHosted,
    Operated,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub release: Version,
    pub deployment_mode: DeploymentMode,
    pub modules: BTreeMap<String, ModuleInstallation>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleInstallation {
    pub version: Version,
    pub configuration: serde_yaml::Value,
}

#[derive(Clone, Debug)]
pub struct ReleaseMetadata {
    pub release: Version,
    pub deployment_modes: BTreeSet<DeploymentMode>,
    pub modules: BTreeMap<String, ModuleMetadata>,
    pub contracts: BTreeMap<String, Version>,
}

#[derive(Clone, Debug)]
pub struct ModuleMetadata {
    pub version: Version,
    pub required_dependencies: BTreeSet<String>,
    pub contract_requirements: BTreeMap<String, VersionReq>,
}

#[derive(Clone, Debug)]
pub struct ValidatedManifest {
    pub deployment_mode: DeploymentMode,
    pub modules: BTreeMap<String, ModuleInstallation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestError {
    message: String,
}

impl ManifestError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ManifestError {}

pub fn supported_release_metadata() -> ReleaseMetadata {
    let mut contract_requirements = BTreeMap::new();
    contract_requirements.insert(
        FOUNDATION_CONTRACT.to_owned(),
        VersionReq::parse("^1.0").expect("static contract requirement is valid"),
    );

    ReleaseMetadata {
        release: Version::new(0, 1, 0),
        deployment_modes: [DeploymentMode::SelfHosted, DeploymentMode::Operated]
            .into_iter()
            .collect(),
        modules: [(
            "example".to_owned(),
            ModuleMetadata {
                version: Version::new(0, 1, 0),
                required_dependencies: BTreeSet::new(),
                contract_requirements,
            },
        )]
        .into_iter()
        .collect(),
        contracts: [(FOUNDATION_CONTRACT.to_owned(), Version::new(1, 0, 0))]
            .into_iter()
            .collect(),
    }
}

pub fn parse_and_validate(
    source: &str,
    metadata: &ReleaseMetadata,
) -> Result<ValidatedManifest, ManifestError> {
    let manifest: Manifest = serde_yaml::from_str(source)
        .map_err(|error| ManifestError::new(format!("manifest is invalid: {error}")))?;
    validate_manifest(manifest, metadata)
}

pub fn validate_manifest(
    manifest: Manifest,
    metadata: &ReleaseMetadata,
) -> Result<ValidatedManifest, ManifestError> {
    if manifest.release != metadata.release {
        return Err(ManifestError::new(format!(
            "release {} is unsupported; this binary supports {}",
            manifest.release, metadata.release
        )));
    }

    if !metadata
        .deployment_modes
        .contains(&manifest.deployment_mode)
    {
        return Err(ManifestError::new(format!(
            "deployment mode {:?} is unsupported by release {}",
            manifest.deployment_mode, metadata.release
        )));
    }

    for (module_id, installation) in &manifest.modules {
        let module = metadata.modules.get(module_id).ok_or_else(|| {
            ManifestError::new(format!("module '{module_id}' is unknown or unsupported"))
        })?;

        if installation.version != module.version {
            return Err(ManifestError::new(format!(
                "module '{module_id}' version {} is incompatible; release {} supports {}",
                installation.version, metadata.release, module.version
            )));
        }

        for (contract_id, requirement) in &module.contract_requirements {
            let contract_version = metadata.contracts.get(contract_id).ok_or_else(|| {
                ManifestError::new(format!(
                    "module '{module_id}' requires missing contract '{contract_id}'"
                ))
            })?;
            if !requirement.matches(contract_version) {
                return Err(ManifestError::new(format!(
                    "module '{module_id}' requires contract '{contract_id}' {requirement}, but release {} provides {contract_version}",
                    metadata.release
                )));
            }
        }
    }

    for module_id in manifest.modules.keys() {
        let metadata = &metadata.modules[module_id];
        for dependency in &metadata.required_dependencies {
            if !module_exists(&manifest.modules, dependency) {
                return Err(ManifestError::new(format!(
                    "module '{module_id}' requires enabled module '{dependency}'"
                )));
            }
        }
    }

    ensure_acyclic(&manifest.modules, metadata)?;

    Ok(ValidatedManifest {
        deployment_mode: manifest.deployment_mode,
        modules: manifest.modules,
    })
}

fn module_exists(modules: &BTreeMap<String, ModuleInstallation>, module_id: &str) -> bool {
    modules.contains_key(module_id)
}

fn ensure_acyclic(
    modules: &BTreeMap<String, ModuleInstallation>,
    metadata: &ReleaseMetadata,
) -> Result<(), ManifestError> {
    fn visit(
        module_id: &str,
        modules: &BTreeMap<String, ModuleInstallation>,
        metadata: &ReleaseMetadata,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), ManifestError> {
        if visited.contains(module_id) {
            return Ok(());
        }
        if !visiting.insert(module_id.to_owned()) {
            return Err(ManifestError::new(format!(
                "module dependency cycle includes '{module_id}'"
            )));
        }

        for dependency in &metadata.modules[module_id].required_dependencies {
            if modules.contains_key(dependency) {
                visit(dependency, modules, metadata, visiting, visited)?;
            }
        }

        visiting.remove(module_id);
        visited.insert(module_id.to_owned());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for module_id in modules.keys() {
        visit(module_id, modules, metadata, &mut visiting, &mut visited)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_and_validate, supported_release_metadata};

    const VALID: &str = "release: 0.1.0\ndeployment_mode: self_hosted\nmodules:\n  example:\n    version: 0.1.0\n    configuration:\n      label: Demo\n";

    #[test]
    fn manifest_validation_accepts_supported_example_installation() {
        let validated = parse_and_validate(VALID, &supported_release_metadata()).unwrap();
        assert!(validated.modules.contains_key("example"));
    }

    #[test]
    fn manifest_validation_rejects_unknown_module_id() {
        let source = VALID.replace("example:", "future-module:");
        let error = parse_and_validate(&source, &supported_release_metadata()).unwrap_err();
        assert!(error.to_string().contains("unknown or unsupported"));
    }

    #[test]
    fn manifest_validation_rejects_unsupported_release_and_module_versions() {
        let release_error = parse_and_validate(
            &VALID.replace("0.1.0", "9.0.0"),
            &supported_release_metadata(),
        )
        .unwrap_err();
        assert!(
            release_error
                .to_string()
                .contains("release 9.0.0 is unsupported")
        );

        let module_error = parse_and_validate(
            &VALID.replace("version: 0.1.0", "version: 9.0.0"),
            &supported_release_metadata(),
        )
        .unwrap_err();
        assert!(
            module_error
                .to_string()
                .contains("version 9.0.0 is incompatible")
        );
    }

    #[test]
    fn generic_manifest_accepts_opaque_module_configuration() {
        let source = VALID.replace("label: Demo", "arbitrary: [1, true, null]");
        let validated = parse_and_validate(&source, &supported_release_metadata()).unwrap();
        assert_eq!(
            validated.modules["example"].configuration,
            serde_yaml::from_str::<serde_yaml::Value>("{arbitrary: [1, true, null]}").unwrap()
        );
    }

    #[test]
    fn generic_manifest_rejects_missing_configuration() {
        let missing_config = VALID.replace("    configuration:\n      label: Demo\n", "");
        assert!(parse_and_validate(&missing_config, &supported_release_metadata()).is_err());
    }

    #[test]
    fn manifest_validation_rejects_unsupported_deployment_mode() {
        let source = VALID.replace("self_hosted", "serverless");
        let error = parse_and_validate(&source, &supported_release_metadata()).unwrap_err();
        assert!(error.to_string().contains("manifest is invalid"));
    }

    #[test]
    fn manifest_validation_rejects_missing_dependency_and_cycles() {
        let mut metadata = supported_release_metadata();
        metadata
            .modules
            .get_mut("example")
            .unwrap()
            .required_dependencies
            .insert("base".into());
        let error = parse_and_validate(VALID, &metadata).unwrap_err();
        assert!(error.to_string().contains("requires enabled module 'base'"));

        metadata
            .modules
            .get_mut("example")
            .unwrap()
            .required_dependencies = ["example".to_owned()].into();
        let error = parse_and_validate(VALID, &metadata).unwrap_err();
        assert!(error.to_string().contains("dependency cycle"));
    }

    #[test]
    fn manifest_validation_rejects_incompatible_contract_metadata() {
        let mut metadata = supported_release_metadata();
        metadata.contracts.get_mut("foundation").unwrap().major = 2;
        let error = parse_and_validate(VALID, &metadata).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires contract 'foundation' ^1.0")
        );
    }
}
