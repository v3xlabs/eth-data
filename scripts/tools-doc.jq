# Renders the compiled tool catalogue as markdown. Driven by `just docs`.
def typename:
  (.type // (.anyOf // [] | map(.type) | first) // "object")
  | if type == "array" then (map(select(. != "null")) | join(" or ")) else . end;

.[] as $tool
| ($tool.inputSchema.required // []) as $required
| "## `\($tool.name)`\n\n\($tool.description // "" | gsub("\n"; " "))\n\n"
  + (
      ($tool.inputSchema.properties // {})
      | to_entries
      | if length == 0 then "Takes no arguments.\n"
        else
          "| Argument | Type | Required | Description |\n| --- | --- | --- | --- |\n"
          + (map(
              "| `\(.key)` | \(.value | typename) | \(if (.key | IN($required[])) then "yes" else "no" end) | \(.value.description // "" | gsub("\n"; " ")) |"
            ) | join("\n"))
          + "\n"
        end
    )
