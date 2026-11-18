program SecureWallet;
{$R 'assets\wallet.res'}
uses Winapi.Windows, System.SysUtils, System.IOUtils, Vcl.Forms,
  SecureWalletApi in 'units\SecureWalletApi.pas',
  SecretEdit in 'units\SecretEdit.pas',
  WalletTheme in 'units\WalletTheme.pas',
  TransferPanel in 'forms\TransferPanel.pas',
  SolanaForm in 'forms\SolanaForm.pas',
  TronForm in 'forms\TronForm.pas',
  AccountsForm in 'forms\AccountsForm.pas',
  SetupForm in 'forms\SetupForm.pas';
function SetDpiContext(Value: NativeInt): BOOL; stdcall;
  external 'user32.dll' name 'SetProcessDpiAwarenessContext';
var Window: TSetupForm;
begin
  SetDpiContext(-4);
  Application.Initialize;
  Application.MainFormOnTaskbar := True;
  Application.Title := 'Web3Rust';
  try
    Application.CreateForm(TSetupForm, Window);
    if SameText(ParamStr(1), '--self-test') then
    begin
      Window.RunSelfTest;
      Window.Free;
      WriteLn('PASS: isolated VCL setup and lock lifecycle');
    end
    else if SameText(ParamStr(1), '--tron-network-self-test') then
    begin
      Window.RunTronNetworkSelfTest;
      Window.Free;
    end
    else if SameText(ParamStr(1), '--network-self-test') then
    begin
      Window.RunNetworkSelfTest;
      Window.Free;
    end
    else Application.Run;
  except
    on E: Exception do
    begin
      if SameText(ParamStr(1), '--network-self-test') or SameText(ParamStr(1), '--tron-network-self-test') then
      begin
        TFile.WriteAllText(TPath.Combine(GetEnvironmentVariable('TEMP'), 'public-network-self-test.txt'), E.Message);
        ExitCode := 1;
      end
      else if SameText(ParamStr(1), '--self-test') then WriteLn('Setup test failed: ', E.Message)
      else MessageBox(0, PChar(E.Message), 'Web3Rust', MB_OK or MB_ICONERROR);
      ExitCode := 1;
    end;
  end;
end.
