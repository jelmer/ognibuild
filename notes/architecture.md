Upstream requirements are expressed as objects derived from UpstreamRequirement.

They can either be:

 * extracted from the build system
 * extracted from errors in build logs

The details of UpstreamRequirements are specific to the kind of requirement,
and otherwise opaque to ognibuild.

When building a package, we first make sure that all declared upstream
requirements are met.

Then we attempt to build.

If any problems are found in the log, buildlog-consultant will report them.

ognibuild can then invoke "fixers" to address Problems.

Problems can be converted to UpstreamRequirements by UpstreamRequirementFixer

Other Fixer can do things like e.g. upgrade configure.ac to a newer version.

UpstreamRequirementFixer uses a UpstreamRequirementResolver object that
can translate UpstreamRequirement objects into apt package names or
e.g. cpan commands.

ognibuild keeps finding problems, resolving them and rebuilding until it finds
a problem it can not resolve or that it thinks it has already resolved
(i.e. seen before).
