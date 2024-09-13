check:: style

style:
	ruff check py

check:: testsuite

build-inplace:
	python3 setup.py build_rust --inplace

testsuite:
	cargo test

check:: typing

typing:
	mypy py tests

coverage:
	PYTHONPATH=$(shell pwd)/py python3 -m coverage run -m unittest tests.test_suite

coverage-html:
	python3 -m coverage html
