#!/bin/env python3

import socket


def main(port=8080):
    address = ("localhost", port)
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(address)

    print("listening on {}".format(address))
    sock.listen(1)

    while True:
        print("waiting for connection\n" + "-" * 10)
        conn, client_addr = sock.accept()
        try:
            # handshake
            conn.sendall(b"\x19Bittorrent protocol")
            conn.sendall(b"\x00" * 8)
            conn.sendall(b"\x01" * 20)  # fake hash
            conn.sendall(b"-PB0001-" + b"\x00" * 12)  # fake peer id

            data = recv_all(20 + 8 + 20 + 20, conn)
            if not data:
                break
            else:
                print("HANDSHAKE: {} {}\n".format(len(data), data.hex()))

            protocol = data[0:20]
            flags = data[20:28]
            file_hash = data[28:48]
            peer_id = data[48:]

            if protocol != b"\x19Bittorrent protocol":
                print("Not bittorrent protocoll [{}]".format(protocol.hex()))

            if flags != b"\x00" * 8:
                print("Flags not empty [{}]".format(flags.hex()))

            print("Serving {} to {}".format(file_hash.hex(), peer_id.hex()))

            # echo
            while True:
                data = conn.recv(16)
                if data:
                    conn.sendall(data)
                else:
                    break
        except ConnectionError:
            pass
        finally:
            print("Disconnected from {}".format(address))
            conn.close()


def recv_all(buf_size: int, conn: socket) -> bytes:
    remaining = buf_size
    res = bytes()
    while remaining > 0:
        data = conn.recv(remaining)
        remaining -= len(data)
        res += data

    return res


if __name__ == "__main__":
    main()
