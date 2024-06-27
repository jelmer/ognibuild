check:: style

style:
	ruff check py tests

check:: testsuite

build-inplace:
	python3 setup.py build_rust --inplace

testsuite: build-inplace
	PYTHONPATH=$(shell pwd)/py python3 -m unittest tests.test_suite

check:: typing

typing:
	mypy py tests

coverage:
	PYTHONPATH=$(shell pwd)/py python3 -m coverage run -m unittest tests.test_suite

coverage-html:
	python3 -m coverage html
