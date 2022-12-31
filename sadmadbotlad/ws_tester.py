import websocket

ws = websocket.WebSocket()

ws.connect("ws://localhost:3000")

print(ws.recv())
