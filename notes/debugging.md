# Debugging

Hopefully ognibuild just Does The Right Thing™, but sometimes it doesn't. Here
are some tips for debugging.

## Detecting dependencies

If you're trying to build a project and it's failing, it might be because
ognibuild is missing a dependency. You can use ``ogni info``
to see what dependencies ognibuild thinks are missing.

## Log file parsing

If a build fails, ognibuild will attempt to parse the log file with
[buildlog-consultant](https://github.com/jelmer/buildlog-consultant)
to try to find out how to fix the build. If you think it's not doing a good job,
you can run buildlog-consultant manually on the log file, and then
possibly file a bug against buildlog-consultant.

## Failure to build

If onibuild fails to determine how to build a project, it will print out
an error message. If you think it should be able to build the project,
please file a bug.

## Reporting bugs

If you think you've found a bug in ognibuild, please report it! You can do so
on GitHub at https://github.com/jelmer/ognibuild/issues/new

If ognibuild crashed, please include the backtrace with
``RUST_BACKTRACE=full`` set.

If it is possible to reproduce the bug on a particular
open source project, please include the URL of that project,
and the exact command you ran.
