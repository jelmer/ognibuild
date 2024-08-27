Ognibuild aims to build and extract build metadata from any software project. It does this through a variety of mechanisms:

* Detecting know build systems
* Ensuring necessary dependencies are present
* Fixing other issues in the project

A build system is anything that can create artefacts from source and/or test
and install those artefacts. Some projects use multiple build systems which may
or may not be tightly integrated.

A build action is one of “clean”, “build”, “install” or “test”.

DependencyCategory: Dependencies can be for different purposes: “build” (just necessary for building), “runtime” (to run after it has been built and possibly installed), “test” (for running tests - e.g. test frameworks, test runners), “dev” (necessary for development - e.g. listens, ide plugins, etc).

When a build action is requested, ognibuild detects the build system(s) and then invokes the action. The action is run and if it failed the output is scanned for problems by buildlog-consultant. 

If a problem is found then the appropriate Fixer is invoked. This may take any of a number of steps, including changing the project source tree, configuring a tool locally for the user or installing more packages.
If no appropriate Fixer can be found then no further action is taken. If the Fixer is successful then the original action is retried.

When it comes to dependencies, there is usually only one relevant fixer loaded. Depending on the situation, this can either update the local project to reference the extra dependencies or install them on the system, invoking the appropriate Installer, Dependency fixers start by trying to derive the missing dependency from the problem that was found. Some second level dependency fixers may then try to coerce the dependency into a specific kind of dependency (e.g. a Debian dependency from a Python dependency).

InstallationScope: Where dependencies are installed can vary from “user” (installed in the user’s home directory), “system” (installed globally on the system, usually in /usr), “vendor” (bundled with the project source code). Not all installers support all scopes. 
