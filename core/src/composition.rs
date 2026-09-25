use std::collections::BTreeMap;

use better_commerce_example::ExampleModule;

use crate::manifest::{ManifestError, ValidatedManifest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComposedModule {
    Example(ExampleModule),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Composition {
    modules: BTreeMap<String, ComposedModule>,
}

impl Composition {
    pub fn enabled_module_ids(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    pub fn module(&self, module_id: &str) -> Option<&ComposedModule> {
        self.modules.get(module_id)
    }
}

pub fn compose_modules(manifest: ValidatedManifest) -> Result<Composition, ManifestError> {
    let mut modules = BTreeMap::new();

    for (module_id, installation) in manifest.modules {
        let module = match module_id.as_str() {
            "example" => ComposedModule::Example(ExampleModule::new(installation.configuration)),
            _ => {
                return Err(ManifestError::new(format!(
                    "module '{module_id}' has no startup composition in this release"
                )));
            }
        };
        modules.insert(module_id, module);
    }

    Ok(Composition { modules })
}

#[cfg(test)]
mod tests {
    use super::{ComposedModule, compose_modules};
    use crate::manifest::{parse_and_validate, supported_release_metadata};
    use better_commerce_example::ExampleModule;

    #[test]
    fn module_composition_contains_only_enabled_modules_and_resolved_configuration() {
        let source = "release: 0.1.0\ndeployment_mode: self_hosted\nmodules:\n  example:\n    version: 0.1.0\n    configuration:\n      label: Demo\n";
        let manifest = parse_and_validate(source, &supported_release_metadata()).unwrap();
        let composition = compose_modules(manifest).unwrap();

        assert_eq!(
            composition.enabled_module_ids().collect::<Vec<_>>(),
            ["example"]
        );
        assert_eq!(
            composition.module("example"),
            Some(&ComposedModule::Example(ExampleModule::new(
                better_commerce_example::ExampleConfiguration {
                    label: "Demo".into()
                }
            )))
        );
        assert!(composition.module("disabled-module").is_none());
    }

    #[test]
    fn module_composition_supports_an_installation_with_no_enabled_modules() {
        let source = "release: 0.1.0\ndeployment_mode: operated\nmodules: {}\n";
        let manifest = parse_and_validate(source, &supported_release_metadata()).unwrap();
        let composition = compose_modules(manifest).unwrap();
        assert_eq!(composition.enabled_module_ids().count(), 0);
    }
}
