// Modo de Batch do Windows (`.bat`, `.cmd`) para o StreamLanguage: nem `@codemirror/language-data` nem `legacy-modes` têm Batch.
// Os nomes dos tokens são tags do @lezer/highlight (comment, labelName, keyword, variableName, string, number, operator), então
// usam as cores do tema de sintaxe do editor sem tabela própria.
import type { StreamParser } from '@codemirror/language';

const KEYWORDS = new Set((
  'echo set setlocal endlocal if else for in do goto call exit not exist defined errorlevel shift pushd popd cd chdir start title pause '
  + 'cls del copy move mkdir md rmdir rd type enabledelayedexpansion enableextensions off on equ neq lss leq gtr geq'
).split(' '));

// `start`: a instrução começa na linha (depois de espaços e de `@`); só ali valem `REM`, `::` e `:rótulo`. `string`: dentro de aspas.
type State = { start: boolean; string: boolean };

const VARIABLE = /%%~?[a-z]+|%~[a-z$:]*\d|%[\d*]|%[^%\s"]+%|![^!\s"]+!/i;

export const batch: StreamParser<State> = {
  name: 'batch',
  startState: () => ({ start: true, string: false }),
  copyState: (state) => ({ ...state }),
  token(stream, state) {
    if (stream.sol()) {
      state.start = true;
      state.string = false;
    }
    if (state.string) {
      if (stream.match(VARIABLE)) return 'variableName';
      if (stream.eat('"')) state.string = false;
      else if (!stream.match(/[^"%!]+/)) stream.next();
      return 'string';
    }
    if (stream.eatSpace()) return null;
    if (state.start) {
      if (stream.eat('@')) return 'operator';
      state.start = false;
      if (stream.match('::')) {
        stream.skipToEnd();
        return 'comment';
      }
      if (stream.match(/rem\b/i)) {
        stream.skipToEnd();
        return 'comment';
      }
      if (stream.match(/:[^\s&|<>]*/)) return 'labelName';
    }
    if (stream.eat('"')) {
      state.string = true;
      return 'string';
    }
    if (stream.match(VARIABLE)) return 'variableName';
    if (stream.match(/&&|\|\||>>|==|[|&<>@]/)) return 'operator';
    if (stream.match(/[^\s%!"&|<>=@()]+/)) {
      const word = stream.current();
      if (/^\d+$/.test(word)) return 'number';
      return KEYWORDS.has(word.toLowerCase()) ? 'keyword' : null;
    }
    stream.next();
    return null;
  },
  languageData: { commentTokens: { line: 'REM' } },
};
