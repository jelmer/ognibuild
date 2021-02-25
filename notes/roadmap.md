class UpstreamRequirement(object):

    family: str


class PythonPackageRequirement(UpstreamRequirement):

    package: str


SetupPy.get_build_requirements() yields some PythonPackageRequirement objects

apt_resolver.install([PythonPackageRequirement(...)]) then:

 * needs to translate to apt package name


Once we find errors during build, buildlog consultant extracts them ("MissingPythonPackage", "configure.ac needs updating").

fix_build then takes the problem found and converts it to an action:

 * modifying some of the source files
 * resolving requirements

Resolving requirements dependencies means creating e.g. a PythonPackageRequirement() object and feeding it to resolver.install()

we have specific handlers for each kind of thingy

resolver.install() needs to translate the upstream information to an apt name or a cpan name or update dependencies or raise an exception or..

MissingPythonPackage() -> PythonPackageRequirement()

PythonPackageRequirement() can either:

 * directly provide apt names, if they are known
 * look up apt names

We specifically want to support multiple resolvers. In some cases a resolver can't deal with a particular kind of requirement.

Who is responsible for taking a PythonPackageRequirement and translating it to an apt package name?

 1) PythonPackageRequirement itself? That would mean knowledge about package naming etc, is with the requirement object, which seems wrong.
 2) PythonPackageRequirement.apt_name(apt_archive) - i.e. find the package name given an archive object of some sort
 3) The apt resolver has a list of callbacks to map requirements to apt package names
