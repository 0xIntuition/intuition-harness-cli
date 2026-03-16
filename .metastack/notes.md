A list of general improvements we need to make to the codebase:

- For the text field, Enter should submit the form, not just add a new line. We want to use Shift+Enter to add a new line.
- When run the `meta listen` + meta backlog tech + meta backlog sync commands there is a long lag before the TUI opens. We should quickly open the TUI and then start listening for events in the background, and showing a loading state while we wait for data from Linear. Additionally, we want the TUI interface to refresh every second, separate from the Linear refresh rate.
- 

