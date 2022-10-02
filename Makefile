check:: style

style:
	flake8

check:: testsuite

testsuite:
	python3 -m unittest ognibuild.tests.test_suite

check:: typing

typing:
	mypy ognibuild

coverage:
	python3 -m coverage run -m unittest ognibuild.tests.test_suite

coverage-html:
	python3 -m coverage html
