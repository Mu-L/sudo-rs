# because the sudo process is `spawn`-ed it may not be immediately visible so
# retry `pidof` until it becomes visible
for _ in $(seq 1 20); do
    # when sudo runs with `use_pty` there are two sudo processes as sudo spawns
    # a monitor process. We want the PID of the sudo process so we assume it
    # must be the smallest of the returned PIDs. 
    sudopid=$(pidof sudo | tr ' ' '\n' | sort -g | head -n1)

	if [ -n "$sudopid" ]; then
        # give `expects-signal.sh ` some time to execute the `trap` command
        # otherwise it'll be terminated before the signal handler is installed
		sleep 0.1
		kill $1 "$sudopid"
		exit 0
	fi
	sleep 0.1
done

echo >&2 timeout
exit 1
