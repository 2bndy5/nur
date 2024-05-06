_comp_cmd_nur()
{
    local cur prev words cword opts
    if type _get_comp_words_by_ref &>/dev/null; then
        _get_comp_words_by_ref -n : cur prev words cword
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"
        words=$COMP_WORDS
        cword=$COMP_CWORD
    fi

    local has_task=0
    for word in "${words[@]}"
    do
        case $word in
            -*|nur)
                ;;
            *)
                has_task=1
                ;;
        esac
    done

    if [[ $has_task -eq 0 ]]
    then
        if [[ ${cur} == -* ]]
        then
            opts=" -h --help --version --list --quiet --stdin"
            COMPREPLY=( $( compgen -W "${opts}" -- "${cur}" ) )
            return 0
        else
            local tasks=$( nur --list )
            local tasks=$( printf "%s\t" $tasks )
            COMPREPLY=( $( compgen -W "${tasks}" -- "${cur}" ) )
        fi
    else
        COMPREPLY=("FUCK")
    fi
} &&
    complete -F _comp_cmd_nur nur
