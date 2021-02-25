Upstream requirements are expressed as objects derived from UpstreamRequirement.

They can either be:

 * extracted from the build system
 * extracted from errors in build logs

The details of UpstreamRequirements are specific to the kind of requirement,
and otherwise opaque to ognibuild.

When building a package, we first make sure that all declared upstream
requirements are met.

Then we attempt to build.

If any Problems are found in the log, buildlog-consultant will report them.

ognibuild can then invoke "fixers" to address Problems. Fixers can do things
like e.g. upgrade configure.ac to a newer version, or invoke autoreconf.

A list of possible fixers can be provided. Each fixer will be called
(in order) until one of them claims to ahve fixed the issue.

Problems can be converted to UpstreamRequirements by UpstreamRequirementFixer

UpstreamRequirementFixer uses a UpstreamRequirementResolver object that
can translate UpstreamRequirement objects into apt package names or
e.g. cpan commands.

ognibuild keeps finding problems, resolving them and rebuilding until it finds
a problem it can not resolve or that it thinks it has already resolved
(i.e. seen before).

Operations are run in a Session - this can represent a virtualized
environment of some sort (e.g. a chroot or virtualenv) or simply
on the host machine.

For e.g. PerlModuleRequirement, need to be able to:

 * install from apt package
  + DebianInstallFixer(AptResolver()).fix(problem)
 * update debian package (source, runtime, test) deps to include apt package
  + DebianPackageDepFixer(AptResolver()).fix(problem, ('test', 'foo'))
 * suggest command to run to install from apt package
  + DebianInstallFixer(AptResolver()).command(problem)
 * install from cpan
  + CpanInstallFixer().fix(problem)
 * suggest command to run to install from cpan package
  + CpanInstallFixer().command(problem)
 * update source package reqs to depend on perl module
  + PerlDepFixer().fix(problem)
