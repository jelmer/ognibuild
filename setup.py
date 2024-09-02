#!/usr/bin/python3
from setuptools import setup
from setuptools_rust import Binding, RustBin, RustExtension

setup(
    rust_extensions=[
        RustExtension(
            "ognibuild._ognibuild_rs",
            "ognibuild-py/Cargo.toml",
            binding=Binding.PyO3,
        ),
        RustBin(
            "ognibuild-deb",
            "Cargo.toml",
            features = ["cli", "debian"]
        ),
        RustBin(
            "ogni",
            "Cargo.toml",
            features = ["cli"]
        ),
        RustBin(
            "deb-upstream-deps",
            "Cargo.toml",
            features = ["cli", "debian"]
        ),
        RustBin(
            "deb-fix-build",
            "Cargo.toml",
            features = ["cli", "debian"]
        ),
    ],
)
