check:: style

style:
	flake8

check:: testsuite

testsuite:
	python3 -m unittest ognibuild.tests.test_suite

check:: typing

typing:
	mypy ognibuild
