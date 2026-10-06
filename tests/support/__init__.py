"""Expose one repository bootstrap for tests of uninstalled command-line tooling."""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"

# Discovery and direct test execution share the same command-module identities without an installation step.
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
