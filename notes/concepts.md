Requirement
===========

Some sort of constraint about the environment that can be specified and satisfied.

Examples:
* a dependency on version 1.3 of the python package "foo"
* a dependency on the apt package "blah"

Requirements can be discovered from build system metadata files and from build logs.

Different kinds of requirements are subclassed from the main Requirement class.

Output
======

A build artifact that can be produced by a build system, e.g. an
executable file or a Perl module.

Problem
=======

An issue found in a build log by buildlog-consultant.

BuildFixer
==========

Takes a build problem and tries to resolve it in some way.

This can mean changing the project that's being built
(by modifying the source tree), or changing the environment
(e.g. by install packages from apt).

Common fixers:

 + InstallFixer([(resolver, repository)])
 + DebianDependencyFixer(tree, resolver)

Repository
==========

Some sort of provider of external requirements. Can satisfy environment
requirements.

Resolver
========

Can take one kind of upstream requirement and turn it into another. E.g.
converting missing Python modules to apt or pypi packages.
