"""
Install the Nockasm Jupyter kernel.

Usage:
    python -m nockasm_kernel.install          # user install (default)
    python -m nockasm_kernel.install --sys-prefix
"""

import json
import os
import sys
import tempfile
import argparse


KERNEL_NAME = 'nockasm'
DISPLAY_NAME = 'Nockasm'

KERNEL_JSON = {
    "argv": [sys.executable, "-m", "nockasm_kernel.kernel", "-f", "{connection_file}"],
    "display_name": DISPLAY_NAME,
    "language": "nockasm",
    "interrupt_mode": "signal",
    "env": {},
    "metadata": {
        "debugger": False,
        "author": "N. E. Davis",
        "license": "MIT",
        "url": "https://github.com/sigilante/nockasm",
        "help_links": [
            {"text": "Nock Specification",
             "url": "https://nock.is/content/specification/index.html"},
            {"text": "Urbit Documentation", "url": "https://docs.urbit.org"},
        ],
    },
}


def install(user=True, prefix=None):
    from jupyter_client.kernelspec import KernelSpecManager
    with tempfile.TemporaryDirectory() as td:
        spec_dir = os.path.join(td, KERNEL_NAME)
        os.makedirs(spec_dir)
        with open(os.path.join(spec_dir, 'kernel.json'), 'w') as f:
            json.dump(KERNEL_JSON, f, indent=2)
        ksm = KernelSpecManager()
        dest = ksm.install_kernel_spec(
            spec_dir,
            kernel_name=KERNEL_NAME,
            user=user,
            prefix=prefix,
        )
    print(f"Installed {DISPLAY_NAME} kernel to: {dest}")


def main():
    parser = argparse.ArgumentParser(
        description=f"Install the {DISPLAY_NAME} Jupyter kernel"
    )
    parser.add_argument(
        '--sys-prefix', action='store_true',
        help="Install into sys.prefix instead of the user directory"
    )
    args = parser.parse_args()
    if args.sys_prefix:
        install(user=False, prefix=sys.prefix)
    else:
        install(user=True)


if __name__ == '__main__':
    main()
