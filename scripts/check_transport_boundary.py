from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
PROTECTED_MODULES = (ROOT / "modules" / "example", ROOT / "modules" / "example-consumer")
FORBIDDEN_RUST = re.compile(
    r"\b(?:better_commerce_example_transport|prost|tonic)\b"
)
FORBIDDEN_PACKAGES = {
    "better-commerce-example-transport",
    "prost",
    "tonic",
}
DEPENDENCY_SECTIONS = {"dependencies", "dev-dependencies", "build-dependencies"}


def find_forbidden_dependencies(value):
    if not isinstance(value, dict):
        return
    for key, nested in value.items():
        if key in DEPENDENCY_SECTIONS and isinstance(nested, dict):
            for alias, specification in nested.items():
                dependency_package = (
                    specification.get("package", alias)
                    if isinstance(specification, dict)
                    else alias
                )
                if dependency_package in FORBIDDEN_PACKAGES:
                    yield dependency_package
        else:
            yield from find_forbidden_dependencies(nested)


def main() -> int:
    violations = []
    for module in PROTECTED_MODULES:
        manifest = module / "Cargo.toml"
        if manifest.exists():
            package = tomllib.loads(manifest.read_text(encoding="utf-8"))
            for dependency_package in find_forbidden_dependencies(package):
                violations.append(
                    f"{manifest.relative_to(ROOT)} depends on forbidden package "
                    f"'{dependency_package}'"
                )
        for source in module.rglob("*.rs"):
            if FORBIDDEN_RUST.search(source.read_text(encoding="utf-8")):
                violations.append(f"{source.relative_to(ROOT)} imports transport/generated protobuf code")

    if violations:
        print("Generated protobuf DTOs must stay outside domain/application code:", file=sys.stderr)
        print("\n".join(f"- {violation}" for violation in violations), file=sys.stderr)
        return 1
    print("Transport boundary check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
