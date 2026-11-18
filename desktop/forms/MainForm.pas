unit MainForm;


interface

uses
  Winapi.Windows, Winapi.Messages, System.SysUtils, System.Classes, System.JSON,
  System.IOUtils, Vcl.Forms, Vcl.Controls, Vcl.StdCtrls, Vcl.ExtCtrls,
  Vcl.ComCtrls, Vcl.Graphics, WalletApi;

type
  TWalletForm = class(TForm)
  private
    FApi: TWalletApi;
    FPages: array[0..3] of TFrame;
    FBody: TPanel;
    FStatus, FError: TLabel;
    FAssets, FHistory: TListView;
    FAsset: TComboBox;
    FAddress, FAmount: TLabeledEdit;
    FConfirm: TMemo;
    FTimer: TTimer;
    FTasks: array[0..1] of UInt64;
    FPage: Integer;
    FLocked: Boolean;
    FLockButton, FRefreshButton, FPreviewButton: TButton;
    procedure Navigate(Sender: TObject);
    procedure ChangeInput(Sender: TObject);
    procedure PreviewClick(Sender: TObject);
    procedure RefreshClick(Sender: TObject);
    procedure TimerTick(Sender: TObject);
    procedure LockClick(Sender: TObject);
    procedure ShowPage(Index: Integer);
    procedure SetLocked(Value: Boolean);
    procedure WmSessionChange(var Msg: TMessage); message $02B1;
    function MakeButton(ParentControl: TWinControl; const CaptionText: string; Handler: TNotifyEvent): TButton;
    procedure Heading(ParentControl: TWinControl; const Text: string);

    function EvidencePath: string;
  public
    constructor Create(AOwner: TComponent); override;
    destructor Destroy; override;
    procedure RunSelfTest;
    procedure WriteTestFailure(const Text: string);
  end;

implementation

function WTSRegisterSessionNotification(hWnd: HWND; dwFlags: DWORD): BOOL; stdcall;
  external 'wtsapi32.dll';
function WTSUnRegisterSessionNotification(hWnd: HWND): BOOL; stdcall;
  external 'wtsapi32.dll';

function TWalletForm.MakeButton(ParentControl: TWinControl; const CaptionText: string; Handler: TNotifyEvent): TButton;
begin
  Result := TButton.Create(Self);
  Result.Parent := ParentControl;
  Result.Caption := CaptionText;
  Result.Align := alTop;
  Result.Height := 42;
  Result.AlignWithMargins := True;
  Result.Margins.Bottom := 12;
  Result.OnClick := Handler;
end;

procedure TWalletForm.Heading(ParentControl: TWinControl; const Text: string);
var L: TLabel;
begin
  L := TLabel.Create(Self);
  L.Parent := ParentControl;
  L.Align := alTop;
  L.AutoSize := False;
  L.Height := 42;
  L.Caption := Text;
  L.Font.Size := 16;
  L.Font.Style := [fsBold];
end;

constructor TWalletForm.Create(AOwner: TComponent);
var
  Header, Nav, Footer: TPanel;
  L: TLabel;
  B: TButton;
  I: Integer;
  Item: TListItem;
  Field: TLabeledEdit;
begin
  inherited CreateNew(AOwner);
  Caption := 'Web3Rust · 桌面原型';
  Font.Name := 'Microsoft YaHei UI';
  Font.Size := 11;
  Color := clWhite;
  Position := poScreenCenter;
  ClientWidth := 960;
  ClientHeight := 720;
  Constraints.MinWidth := 800;
  Constraints.MinHeight := 600;
  Header := TPanel.Create(Self);
  Header.Parent := Self;
  Header.Align := alTop;
  Header.Height := 88;
  Header.BevelOuter := bvNone;
  Header.Padding.SetBounds(24, 12, 24, 8);
  Heading(Header, 'Web3Rust  /  演示钱包');
  L := TLabel.Create(Self);
  L.Parent := Header;
  L.Align := alBottom;
  L.Caption := '模拟数据 · 未联网 · 不接收真实密钥，不签名或广播';
  L.Font.Color := $00605020;
  Footer := TPanel.Create(Self);
  Footer.Parent := Self;
  Footer.Align := alBottom;
  Footer.Height := 44;
  Footer.BevelOuter := bvNone;
  Footer.Padding.SetBounds(24, 8, 24, 8);
  FStatus := TLabel.Create(Self);
  FStatus.Parent := Footer;
  FStatus.Align := alClient;
  FStatus.Caption := '离线演示就绪；金额与地址由 Rust 校验';
  Nav := TPanel.Create(Self);
  Nav.Parent := Self;
  Nav.Align := alLeft;
  Nav.Width := 170;
  Nav.BevelOuter := bvNone;
  Nav.Padding.SetBounds(16, 16, 16, 16);
  B := MakeButton(Nav, '我的资产', Navigate); B.Tag := 0;
  B := MakeButton(Nav, '交易记录', Navigate); B.Tag := 1;
  B := MakeButton(Nav, '转账预览', Navigate); B.Tag := 2;
  FRefreshButton := MakeButton(Nav, '检查本地进程', RefreshClick);
  FLockButton := MakeButton(Nav, '锁定演示', LockClick);
  FBody := TPanel.Create(Self);
  FBody.Parent := Self;
  FBody.Align := alClient;
  FBody.BevelOuter := bvNone;
  FBody.Padding.SetBounds(24, 16, 24, 16);
  for I := 0 to 3 do
  begin
    FPages[I] := TFrame.Create(Self);
    FPages[I].Parent := FBody;
    FPages[I].Align := alClient;
    FPages[I].Visible := False;
  end;

  FAssets := TListView.Create(Self);
  FAssets.Parent := FPages[0];
  FAssets.Align := alClient;
  FAssets.ViewStyle := vsReport;
  FAssets.ReadOnly := True;
  FAssets.RowSelect := True;
  FAssets.Columns.Add.Caption := '资产'; FAssets.Columns[0].Width := 150;
  FAssets.Columns.Add.Caption := '网络'; FAssets.Columns[1].Width := 160;
  FAssets.Columns.Add.Caption := '模拟余额'; FAssets.Columns[2].Width := 230;
  Item := FAssets.Items.Add; Item.Caption := 'DEMO-USDT'; Item.SubItems.Add('BSC · BEP-20'); Item.SubItems.Add('1,250.00');
  Item := FAssets.Items.Add; Item.Caption := 'BNB'; Item.SubItems.Add('BSC'); Item.SubItems.Add('0.05');
  Item := FAssets.Items.Add; Item.Caption := 'DEMO-USDT'; Item.SubItems.Add('TRON · TRC-20'); Item.SubItems.Add('800.00');
  Item := FAssets.Items.Add; Item.Caption := 'TRX'; Item.SubItems.Add('TRON'); Item.SubItems.Add('120.00');
  Heading(FPages[0], '我的资产 · 4 项演示资产');
  FHistory := TListView.Create(Self);
  FHistory.Parent := FPages[1];
  FHistory.Align := alClient;
  FHistory.ViewStyle := vsReport;
  FHistory.ReadOnly := True;
  FHistory.Columns.Add.Caption := '演示资产'; FHistory.Columns[0].Width := 180;
  FHistory.Columns.Add.Caption := '金额'; FHistory.Columns[1].Width := 120;
  FHistory.Columns.Add.Caption := '状态（示例）'; FHistory.Columns[2].Width := 250;
  Item := FHistory.Items.Add; Item.Caption := 'BNB · BSC'; Item.SubItems.Add('-0.01'); Item.SubItems.Add('广播结果未知 · 待核实');
  Item := FHistory.Items.Add; Item.Caption := 'DEMO-USDT · TRON'; Item.SubItems.Add('-10.00'); Item.SubItems.Add('已入块 · 待最终确认');
  Heading(FPages[1], '交易记录 · 虚构状态示例');

  B := MakeButton(FPages[2], '下一步：核对预览', PreviewClick); B.Align := alBottom; FPreviewButton := B;
  FError := TLabel.Create(Self); FError.Parent := FPages[2]; FError.Align := alBottom;
  FError.AutoSize := False; FError.Height := 70; FError.WordWrap := True; FError.Font.Color := clMaroon;
  Heading(FPages[2], '转账预览 · 仅校验公开输入');
  FAsset := TComboBox.Create(Self); FAsset.Parent := FPages[2]; FAsset.SetBounds(0, 70, 420, 32);
  FAsset.Style := csDropDownList;
  FAsset.Items.Add('BNB · BSC'); FAsset.Items.Add('DEMO-USDT · BSC');
  FAsset.Items.Add('TRX · TRON'); FAsset.Items.Add('DEMO-USDT · TRON');
  FAsset.ItemIndex := 0; FAsset.OnChange := ChangeInput;
  Field := TLabeledEdit.Create(Self); Field.Parent := FPages[2]; Field.SetBounds(0, 150, 600, 34);
  Field.Anchors := [akLeft, akTop, akRight]; Field.EditLabel.Caption := '收款地址（完整地址）';
  Field.MaxLength := 64; Field.OnChange := ChangeInput; FAddress := Field;
  Field := TLabeledEdit.Create(Self); Field.Parent := FPages[2]; Field.SetBounds(0, 230, 600, 34);
  Field.Anchors := [akLeft, akTop, akRight]; Field.EditLabel.Caption := '金额（十进制，不使用科学计数法）';
  Field.MaxLength := 157; Field.OnChange := ChangeInput; FAmount := Field;

  B := MakeButton(FPages[3], '返回修改', Navigate); B.Tag := 2; B.Align := alBottom;
  B := MakeButton(FPages[3], '确认发送（原型未开放）', nil); B.Enabled := False; B.Align := alBottom;
  FConfirm := TMemo.Create(Self); FConfirm.Parent := FPages[3]; FConfirm.Align := alClient;
  FConfirm.ReadOnly := True; FConfirm.WordWrap := True; FConfirm.ScrollBars := ssVertical;
  FConfirm.BorderStyle := bsNone;
  Heading(FPages[3], '核对预览 · 尚未准备交易');
  FApi := TWalletApi.Create(ExtractFilePath(ParamStr(0)));
  FTimer := TTimer.Create(Self); FTimer.Interval := 50; FTimer.OnTimer := TimerTick;
  ShowPage(0);
  WTSRegisterSessionNotification(Handle, 0);
end;

destructor TWalletForm.Destroy;
begin
  if Assigned(FTimer) then FTimer.Enabled := False;
  if HandleAllocated then WTSUnRegisterSessionNotification(Handle);
  FApi.Free;
  inherited;
end;

procedure TWalletForm.ShowPage(Index: Integer);
var I: Integer;
begin
  if FLocked then Exit;
  for I := 0 to 3 do FPages[I].Visible := I = Index;
  FPage := Index;
end;

procedure TWalletForm.Navigate(Sender: TObject);
begin
  FConfirm.Clear;
  ShowPage(TControl(Sender).Tag);
end;

procedure TWalletForm.ChangeInput(Sender: TObject);
begin
  if Assigned(FConfirm) then FConfirm.Clear;
  FError.Caption := '';
end;

procedure TWalletForm.PreviewClick(Sender: TObject);
var
  Text: string;
  Code: Int32;
  Json: TJSONObject;
begin
  if FLocked then Exit;
  FConfirm.Clear;
  Code := FApi.Preview(FAsset.ItemIndex + 1, FAddress.Text, FAmount.Text, Text);
  if Code <> W3_OK then
  begin
    case Code of
      -10: FError.Caption := '地址无效或与所选网络不符，请核对完整地址。';
      -11: FError.Caption := '金额无效：须大于零，且不能超出精度或范围。';
    else FError.Caption := '请完整填写地址和金额。错误码：' + IntToStr(Code);
    end;
    Exit;
  end;
  Json := TJSONObject.ParseJSONValue(Text) as TJSONObject;
  try
    if (Json = nil) or (Json.GetValue<Integer>('schema_version') <> 1) or
      (Json.GetValue<string>('mode') <> 'demo') or Json.GetValue<Boolean>('can_sign') then
      raise Exception.Create('Unexpected preview model');
    FConfirm.Lines.Add('网络：' + Json.GetValue<string>('network'));
    FConfirm.Lines.Add('资产：' + Json.GetValue<string>('asset'));
    FConfirm.Lines.Add('');
    FConfirm.Lines.Add('收款地址：');
    FConfirm.Lines.Add(Json.GetValue<string>('recipient'));
    FConfirm.Lines.Add('');
    FConfirm.Lines.Add('金额：' + Json.GetValue<string>('amount'));
    FConfirm.Lines.Add('');
    FConfirm.Lines.Add('费用：尚未查询，不能估算');
    FConfirm.Lines.Add('余额与网络状态：尚未核实');
    FConfirm.Lines.Add('');
    FConfirm.Lines.Add('本页仅核对输入，不创建发送意图、不签名、不广播。');
    ShowPage(3);
  finally Json.Free; end;
end;

procedure TWalletForm.RefreshClick(Sender: TObject);
begin
  if FLocked or (FTasks[0] <> 0) or (FTasks[1] <> 0) then Exit;
  FRefreshButton.Enabled := False;
  FStatus.Caption := '正在检查本地模拟进程…';
  FTasks[0] := FApi.Submit('signer.ping');
  FTasks[1] := FApi.Submit('network.ping');
end;

procedure TWalletForm.TimerTick(Sender: TObject);
var I: Integer; Code: Int32; Text: string;
begin
  for I := 0 to 1 do
    if FTasks[I] <> 0 then
    begin
      Code := FApi.Poll(FTasks[I], Text);
      if Code <> W3_PENDING then
      begin
        FTasks[I] := 0;
        if not FLocked then
          if Code = W3_OK then FStatus.Caption := Text
          else FStatus.Caption := '本地进程检查未完成；错误码 ' + IntToStr(Code);
      end;
    end;
  FRefreshButton.Enabled := not FLocked and (FTasks[0] = 0) and (FTasks[1] = 0);
end;

procedure TWalletForm.SetLocked(Value: Boolean);
var I: Integer;
begin
  FLocked := Value;
  if Value then
  begin
    FApi.Lock;
    FConfirm.Clear;
    FAddress.Clear;
    FAmount.Clear;
    for I := 0 to 3 do FPages[I].Visible := False;
    FStatus.Caption := '演示已锁定，输入和预览已清除。';
    FLockButton.Caption := '恢复演示';
  end
  else
  begin
    FLockButton.Caption := '锁定演示';
    FStatus.Caption := '已恢复演示；没有解锁真实钱包。';
    ShowPage(0);
  end;
end;

procedure TWalletForm.LockClick(Sender: TObject);
begin
  SetLocked(not FLocked);
end;

procedure TWalletForm.WmSessionChange(var Msg: TMessage);
begin
  if Msg.WParam = 7 then SetLocked(True); // WTS_SESSION_LOCK
end;

function TWalletForm.EvidencePath: string;
begin
  Result := TPath.GetFullPath(TPath.Combine(ExtractFilePath(ParamStr(0)), '..\gui'));
  ForceDirectories(Result);
end;

procedure TWalletForm.WriteTestFailure(const Text: string);
begin
  TFile.WriteAllText(TPath.Combine(EvidencePath, 'self-test.txt'), 'FAIL: ' + Text, TEncoding.UTF8);
end;

procedure TWalletForm.RunSelfTest;
var Scale: Integer; Deadline: UInt64;
  procedure Require(Value: Boolean; const MessageText: string);
  begin
    if not Value then raise Exception.Create(MessageText);
  end;
begin
  // Exercise the same event handlers and real DLL as the interactive prototype.
  Position := poDesigned;
  Left := -10000;
  Show;
  Update;
  for Scale in [96, 120, 144, 192] do
  begin
    ScaleForPPI(Scale);
    ClientWidth := MulDiv(960, Scale, 96);
    ClientHeight := MulDiv(720, Scale, 96);
    ShowPage(0);
    ShowPage(1);
    ShowPage(2);
    FAsset.ItemIndex := 3;
    FAddress.Text := 'TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC';
    FAmount.Text := '1.0000001';
    FPreviewButton.Click; Require(FPage = 2, 'Invalid precision accepted');
    Require(FError.Caption <> '', 'Missing error message');

    FAmount.Text := '0.000001';
    FPreviewButton.Click; Require(FPage = 3, 'Valid preview rejected');
    Require(Pos(FAddress.Text, FConfirm.Text) > 0, 'Full address missing');
    Require(Pos('0.000001', FConfirm.Text) > 0, 'Exact amount missing');

  end;
  RefreshClick(nil);
  Deadline := GetTickCount64 + 5000;
  repeat
    TimerTick(nil);
    Require(GetTickCount64 < Deadline, 'Process polling timeout');
    Sleep(5);
  until (FTasks[0] = 0) and (FTasks[1] = 0);
  SetLocked(True);
  Require((FAddress.Text = '') and (FAmount.Text = '') and (FConfirm.Text = ''), 'Lock did not clear inputs');
  Require(not FPages[0].Visible and not FPages[3].Visible, 'Lock did not hide pages');
  SetLocked(False);
  Require(FPage = 0, 'Restore failed');
  TFile.WriteAllText(TPath.Combine(EvidencePath, 'self-test.txt'),
    'PASS: four pages; 96/120/144/192 PPI behavior; invalid precision; exact preview; full address; real IPC; lock clearing.', TEncoding.UTF8);
end;

end.
