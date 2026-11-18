program Web3Rust;

uses
  Winapi.Windows, System.SysUtils, Vcl.Forms,
  WalletApi in 'units\WalletApi.pas',
  MainForm in 'forms\MainForm.pas';

function SetDpiContext(Value: NativeInt): BOOL; stdcall;
  external 'user32.dll' name 'SetProcessDpiAwarenessContext';

var
  Window: TWalletForm;
begin
  // Set before any window is created. Current prototype uses runtime VCL controls.
  SetDpiContext(-4);
  Application.Initialize;
  Application.MainFormOnTaskbar := True;
  Application.Title := 'Web3Rust';
  Application.CreateForm(TWalletForm, Window);
  if SameText(ParamStr(1), '--self-test') then
  begin
    try
      Window.RunSelfTest;
    except
      on E: Exception do
      begin
        Window.WriteTestFailure(E.Message);
        ExitCode := 1;
      end;
    end;
    Window.Free;
  end
  else Application.Run;
end.
