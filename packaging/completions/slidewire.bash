# bash completion for slidewire
_slidewire() {
    local cur prev
    cur=${COMP_WORDS[COMP_CWORD]}
    prev=${COMP_WORDS[COMP_CWORD-1]}
    if [[ $COMP_CWORD -eq 1 ]]; then
        COMPREPLY=($(compgen -W "strips set mute mic bank banks scene scenes profile profiles state --minimized --list-ports --monitor --doctor --install-icon --uninstall --write-icons --version --help" -- "$cur"))
        return
    fi
    case ${COMP_WORDS[1]} in
        mute) [[ $COMP_CWORD -eq 3 ]] && COMPREPLY=($(compgen -W "on off toggle" -- "$cur")) ;;
        mic) [[ $COMP_CWORD -eq 2 ]] && COMPREPLY=($(compgen -W "on off toggle" -- "$cur")) ;;
        scene) [[ $COMP_CWORD -eq 2 ]] && COMPREPLY=($(compgen -W "save" -- "$cur")) ;;
        set) COMPREPLY=($(compgen -W "--volume" -- "$cur")) ;;
        strips|scenes|banks|profiles) COMPREPLY=($(compgen -W "--json" -- "$cur")) ;;
        --write-icons) COMPREPLY=($(compgen -d -- "$cur")) ;;
    esac
}
complete -F _slidewire slidewire
