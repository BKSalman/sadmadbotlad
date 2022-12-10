import websocket

ws = websocket.WebSocket()

ws.connect("ws://localhost:3000")

# ws.send("Hello, Server")

# print()

while (True):
    print(ws.recv())

# ws.close()
