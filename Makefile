check:: style

style:
	flake8

check:: testsuite

build-inplace:
	python3 setup.py build_rust --inplace

testsuite: build-inplace
	python3 -m unittest tests.test_suite

check:: typing

typing:
	mypy ognibuild tests

coverage:
	python3 -m coverage run -m unittest tests.test_suite

coverage-html:
	python3 -m coverage html
