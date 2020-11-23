# tcptunnelchecker
[WiP] Checks TCP tunnel/forwarder for correctness

```
$ socat tcp-l:5747,fork,reuseaddr tcp:127.0.0.1:5746&
[1] 12504
$ cargo run -- 127.0.0.1:5746 127.0.0.1:5747
    Finished dev [unoptimized + debuginfo] target(s) in 0.01s
     Running `target/debug/tcptunnelchecker '127.0.0.1:5746' '127.0.0.1:5747'`
[ OK ] Trivial test 1
[ OK ] Trivial test 2
The other direction buffer: 9973881
One direction buffer: 10942496
2020/11/24 00:10:07 socat[12598] E write(6, 0x5634a2c54ab0, 8192): Connection reset by peer
[ OK ] Clogged close test 1 passed
2020/11/24 00:10:08 socat[12600] E write(5, 0x5634a2c54ab0, 8192): Connection reset by peer
[ OK ] Clogged close test 2 passed
2020/11/24 00:10:08 socat[12603] E write(5, 0x5634a2c54ab0, 8192): Connection reset by peer
[ OK ] Clogged close test 3 passed
2020/11/24 00:10:08 socat[12605] E write(6, 0x5634a2c54ab0, 8192): Broken pipe
[ OK ] Clogged close test 4 passed
2020/11/24 00:10:09 socat[12607] E write(5, 0x5634a2c54ab0, 8192): Broken pipe
[ OK ] Clogged close test 5 passed
2020/11/24 00:10:09 socat[12610] E write(6, 0x5634a2c54ab0, 8192): Broken pipe
[ OK ] Clogged close test 6 passed
$ kill %%
```
