//! Embedded shell completion scripts for bash, fish, and zsh.
//!
//! Each script assumes `runlatch` is on PATH and calls `runlatch complete entries`
//! / `runlatch complete sources` for dynamic candidates. The static structure
//! (subcommands, flags) is written by hand — small enough that generating it is
//! more complexity than it saves.

pub const BASH: &str = r#"
# runlatch bash completions — source with: source <(runlatch completions bash)

_runlatch() {
    local cur prev words cword
    _init_completion 2>/dev/null || {
        COMPREPLY=()
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"
    }

    local subcommand=""
    local i
    for (( i=1; i < ${#COMP_WORDS[@]}; i++ )); do
        case "${COMP_WORDS[i]}" in
            list|enable|disable|sources|completions) subcommand="${COMP_WORDS[i]}"; break ;;
        esac
    done

    case "$subcommand" in
        "")
            COMPREPLY=($(compgen -W "list enable disable sources completions" -- "$cur"))
            ;;
        enable|disable)
            COMPREPLY=($(compgen -W "$(runlatch complete entries 2>/dev/null)" -- "$cur"))
            ;;
        list)
            case "$prev" in
                --source)
                    COMPREPLY=($(compgen -W "$(runlatch complete sources 2>/dev/null)" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "--source --json" -- "$cur"))
                    ;;
            esac
            ;;
        completions)
            COMPREPLY=($(compgen -W "bash fish zsh" -- "$cur"))
            ;;
    esac
}

complete -F _runlatch runlatch
"#;

pub const FISH: &str = r#"
# runlatch fish completions — install with:
#   runlatch completions fish > ~/.config/fish/completions/runlatch.fish

# Disable file completion globally for runlatch.
complete -c runlatch -f

# Top-level subcommands.
complete -c runlatch -n "not __fish_seen_subcommand_from list enable disable sources completions" \
    -a list        -d "List autostart entries"
complete -c runlatch -n "not __fish_seen_subcommand_from list enable disable sources completions" \
    -a enable      -d "Enable an entry"
complete -c runlatch -n "not __fish_seen_subcommand_from list enable disable sources completions" \
    -a disable     -d "Disable an entry"
complete -c runlatch -n "not __fish_seen_subcommand_from list enable disable sources completions" \
    -a sources     -d "List providers and their availability"
complete -c runlatch -n "not __fish_seen_subcommand_from list enable disable sources completions" \
    -a completions -d "Print a shell completion script"

# enable / disable: complete entry ids.
complete -c runlatch -n "__fish_seen_subcommand_from enable disable" \
    -a "(runlatch complete entries 2>/dev/null)"

# list flags.
complete -c runlatch -n "__fish_seen_subcommand_from list" \
    -l source -d "Filter by source" \
    -a "(runlatch complete sources 2>/dev/null)"
complete -c runlatch -n "__fish_seen_subcommand_from list" \
    -l json -d "Emit JSON"

# completions: shell names.
complete -c runlatch -n "__fish_seen_subcommand_from completions" \
    -a "bash fish zsh"
"#;

pub const ZSH: &str = r#"
#compdef runlatch
# runlatch zsh completions — install with:
#   runlatch completions zsh > "${fpath[1]}/_runlatch"

_runlatch() {
    local state

    _arguments \
        '1: :->subcommand' \
        '*: :->args'

    case $state in
        subcommand)
            local -a cmds
            cmds=(
                'list:List autostart entries'
                'enable:Enable an entry'
                'disable:Disable an entry'
                'sources:List providers and their availability'
                'completions:Print a shell completion script'
            )
            _describe 'subcommand' cmds
            ;;
        args)
            case $words[2] in
                enable|disable)
                    local -a entries
                    entries=(${(f)"$(runlatch complete entries 2>/dev/null)"})
                    _describe 'entry' entries
                    ;;
                list)
                    _arguments \
                        '--json[Emit JSON]' \
                        '--source[Filter by source]:source:(${(f)"$(runlatch complete sources 2>/dev/null)"})'
                    ;;
                completions)
                    _arguments '1:shell:(bash fish zsh)'
                    ;;
            esac
            ;;
    esac
}

_runlatch "$@"
"#;
