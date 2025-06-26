## Sapf-as-plain*-text
**I know it's not true, but I couldn't resist*

An Editor for [SAPF](https://github.com/lfnoise/sapf).
Just the beginning.

### How to use
You must have 'sapf' in your path.
Don't forget to setup your 'sapf' folders:
```
export SAPF_HISTORY="$HOME/sapf-files/sapf-history.txt"
export SAPF_LOG="$HOME/sapf-files/sapf-log.txt"
export SAPF_PRELUDE="$HOME/sapf-files/sapf-prelude.txt"
export SAPF_EXAMPLES="$HOME/sapf-files/sapf-examples.txt"
export SAPF_README="$HOME/sapf-files/README.txt"
export SAPF_RECORDINGS="$HOME/sapf-files/recordings"
export SAPF_SPECTROGRAMS="$HOME/sapf-files/spectrograms"
```

### Keybindings
```
CTRL +
  RETURN - send the current line
  . - stop all sound
  E - stop previous sounds and send the current line
  R - record (it takes care about the file name)
  D - clear the stack
  P - print the stack
  TAB - call completions popup
 ```

### TODO
- [X] More keybindings — stop, reinit, clear stack, record
- [X] Buffers
- [X] Export
- [ ] Make it pretty — optional syntax highlighting, themes, etc
- [ ] Better completions
- [ ] VIM motions?
