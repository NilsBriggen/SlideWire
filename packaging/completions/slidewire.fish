# fish completion for slidewire
set -l commands strips set mute bank scene
complete -c slidewire -f
complete -c slidewire -n "not __fish_seen_subcommand_from $commands" -a strips -d 'List strips with volume and assignment'
complete -c slidewire -n "not __fish_seen_subcommand_from $commands" -a set -d "Set or change a strip's volume"
complete -c slidewire -n "not __fish_seen_subcommand_from $commands" -a mute -d 'Mute or unmute a strip'
complete -c slidewire -n "not __fish_seen_subcommand_from $commands" -a bank -d 'Switch to a bank'
complete -c slidewire -n "not __fish_seen_subcommand_from $commands" -a scene -d 'Recall a scene'
complete -c slidewire -n "__fish_seen_subcommand_from mute" -a 'on off toggle'
complete -c slidewire -l minimized -d 'Start in the tray'
complete -c slidewire -l list-ports -d 'List MIDI ports'
complete -c slidewire -l monitor -d 'Print incoming MIDI'
complete -c slidewire -l doctor -d "Check why a controller doesn't show up"
complete -c slidewire -l install-icon -d 'Install a menu entry and icons'
complete -c slidewire -l uninstall -d 'Remove the menu entry, icons and autostart'
complete -c slidewire -l write-icons -r -a '(__fish_complete_directories)' -d 'Write theme icons'
complete -c slidewire -s V -l version -d 'Print the version'
complete -c slidewire -s h -l help -d 'Show help'
