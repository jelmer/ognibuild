#!/usr/bin/python3
from setuptools import setup
from setuptools_rust import Binding, RustExtension

setup(
    rust_extensions=[
        RustExtension(
            "ognibuild._ognibuild_rs",
            "ognibuild-py/Cargo.toml",
            binding=Binding.PyO3,
        )
    ],
)
