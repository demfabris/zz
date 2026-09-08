function request_id(line) {
  if (match(line, /"id":"[^"]*"/)) {
    return substr(line, RSTART + 5, RLENGTH - 5)
  }
  if (match(line, /"id":[0-9]+/)) {
    return substr(line, RSTART + 5, RLENGTH - 5)
  }
  return "null"
}
/"method":"initialize"/ {
  printf "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"protocolVersion\":1,\"agentInfo\":{\"name\":\"zz-gtk-fixture\",\"version\":\"1\"}}}\n", request_id($0)
  fflush()
  next
}
/"method":"session\/new"/ {
  printf "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"sessionId\":\"gtk-agent-session\"}}\n", request_id($0)
  fflush()
  next
}
/"method":"session\/prompt"/ {
  prompt_id = request_id($0)
  print "{\"jsonrpc\":\"2.0\",\"id\":\"gtk-permission\",\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"gtk-agent-session\",\"toolCall\":{\"toolCallId\":\"gtk-read\",\"title\":\"Read fixture\",\"rawInput\":{\"path\":\"/tmp/gtk-fixture.txt\"},\"kind\":\"read\",\"status\":\"pending\"},\"options\":[{\"optionId\":\"allow\",\"name\":\"Allow once\",\"kind\":\"allow_once\"}]}}"
  fflush()
  next
}
/"id":"gtk-permission"/ {
  print "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"gtk-agent-session\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"gtk-agent-answered\"}}}}"
  printf "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"stopReason\":\"end_turn\"}}\n", prompt_id
  fflush()
  next
}
