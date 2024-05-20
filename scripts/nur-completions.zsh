#compdef nur

_nur_tasks() {
    [[ $PREFIX = -* ]] && return 1
    local tasks; tasks=(
        "${(@f)$(_call_program commands nur --list)}"
    )

    _describe 'nur tasks' tasks
}

_nur() {
    local curcontext="$curcontext" state line ret=1
    typeset -A opt_args

    _arguments -C \
        '-h[Display the help message for this command]' \
        '--help[Display the help message for this command]' \
        '-v[Output version number and exit]' \
        '--version[Output version number and exit]' \
        '-l[List available tasks and then just exit]' \
        '--list[List available tasks and then just exit]' \
        '-q[Do not output anything but what the task produces]' \
        '--quiet[Do not output anything but what the task produces]' \
        '--stdin[Attach stdin to called nur task]' \
        '-c[Run the given commands after nurfiles have been loaded]' \
        '--commands[Run the given commands after nurfiles have been loaded]' \
        '--enter-shell[Enter a nu REPL shell after the nurfiles have been loaded (use only for debugging)]' \
        '::optional arg:_nur_tasks' \
        '*: :->args' \
        && ret=0

    return ret
}

compdef _nur nur
