unit SetupForm;
interface
uses Winapi.Windows, Winapi.Messages, System.SysUtils, System.Classes, System.JSON,
  Vcl.Forms, Vcl.Controls, Vcl.StdCtrls, Vcl.ExtCtrls, Vcl.Graphics, WalletTheme, SecureWalletApi, SecretEdit, TransferPanel;
type
  TSetupForm = class(TForm)
  private
    FApi: TSecureWalletApi;
    FSmallIcon, FLargeIcon: HICON;
    FProfile: TComboBox;
    FPassword, FRepeat, FWords, FBackup: TSecretEdit;
    FStatus, FIntro, FWordsLabel, FBackupLabel: TLabel;
    FTitle, FProfileLabel: TLabel;
    FMark: TWalletMark;
    FTimer: TTimer;
    FDeadline: UInt64;
    FActivityPending: Boolean;
    FBusy, FPending, FNeedsReopen, FViewingBackup: Boolean;
    FExport, FDone, FDelete: TButton;
    FRestoreStep: Integer;
    FNext, FBack: TButton;
    FWordCount: TComboBox;
    FWordInputs: array[0..23] of TSecretEdit;
    FWordNumbers: array[0..23] of TLabel;
    FConfirm, FShowAddress, FCreate, FRestore, FResume, FNew: TButton;
    FPages: TWalletDeck;
    FTransfer: TTransferPanel;
    procedure UserInput(var Msg: TMsg; var Handled: Boolean);
    procedure StartRestore(Sender: TObject);
    procedure WordCountChanged(Sender: TObject);
    procedure RestoreStep(Sender: TObject);
    procedure ApplyWindowIcons;
    procedure UpdateSetup;
    procedure ShowAccount(const Value: UTF8String);
    procedure NewWallet(Sender: TObject);
    procedure ManageWallet(Sender: TObject);
    procedure SelectAccount(Sender: TObject);
    procedure Closing(Sender: TObject; var CanClose: Boolean);
    procedure ProfileChanged(Sender: TObject);
    procedure Action(Sender: TObject);
    procedure Tick(Sender: TObject);
    procedure ClearSecrets;
    procedure LockWallet;
    procedure WmSessionChange(var Msg: TMessage); message $02B1;
    function Button(const Text: string; Op: Integer; X, Y: Integer): TButton;
  protected
    procedure CreateWnd; override;
    procedure ChangeScale(M, D: Integer; isDpiChange: Boolean); override;
  public
    constructor Create(AOwner: TComponent); override;
    destructor Destroy; override;
    procedure RunSelfTest;
    procedure RunNetworkSelfTest;
    procedure RunTronNetworkSelfTest;
    procedure RunBackupFlowSelfTest;
  end;
implementation
uses AccountsForm, SolanaForm, TronForm;
const IdleLockMs: UInt64 = 2 * 60 * 60 * 1000;
function WTSRegisterSessionNotification(hWnd: HWND; dwFlags: DWORD): BOOL; stdcall; external 'wtsapi32.dll';
function WTSUnRegisterSessionNotification(hWnd: HWND): BOOL; stdcall; external 'wtsapi32.dll';
function TSetupForm.Button(const Text: string; Op: Integer; X, Y: Integer): TButton;
begin
  Result := TWalletButton.Create(Self); Result.Parent := Self;
  Result.SetBounds(X, Y, 170, 36); Result.Caption := Text;
  Result.Tag := Op; Result.OnClick := Action;
end;
constructor TSetupForm.Create(AOwner: TComponent);
var L: TLabel; SetupTab, TransferTab: TWalletPage; I: Integer;
    Profiles: TJSONObject; Names: TJSONArray; Response: UTF8String;
begin
  inherited CreateNew(AOwner);
  Caption := 'Web3Rust Wallet'; ClientWidth := 540; ClientHeight := 800;
  BorderStyle := bsSingle; BorderIcons := [biSystemMenu,biMinimize];
  Font.Name := 'Microsoft YaHei UI'; Font.Size := 10; Font.Color := Ink;
  Color := clWhite; Position := poScreenCenter; DoubleBuffered := True;
  L := LabelText(Self,Self,'Web3Rust',32,24,220,32,14,True);
  L := LabelText(Self,Self,'你的钱包，由你掌握',306,28,202,24,9,False,Muted); L.Alignment := taRightJustify;
  FMark := TWalletMark.Create(Self); FMark.Parent := Self; FMark.SetBounds(222,126,96,96);
  FTitle := LabelText(Self,Self,'欢迎回来',40,248,460,48,26,True); FTitle.Alignment := taCenter;
  FIntro := LabelText(Self,Self,'解锁钱包，查看你的资产',40,304,460,44,10,False,Muted); FIntro.Alignment := taCenter;
  FProfileLabel := LabelText(Self,Self,'钱包',40,366,460,22,10,False,Muted);
  FProfile := TComboBox.Create(Self); FProfile.Parent := Self; FProfile.SetBounds(40,394,460,30);
  FProfile.MaxLength := 48; FProfile.Text := 'wallet-' + FormatDateTime('yyyymmdd-hhnnss', Now);
  FPassword := TSecretEdit.Create(Self); FPassword.Parent := Self; FPassword.Font.Size := 12;
  FPassword.SetBounds(40,466,460,32); FPassword.EditLabel.Caption := '钱包密码';
  FPassword.PasswordChar := '*';
  FRepeat := TSecretEdit.Create(Self); FRepeat.Parent := Self; FRepeat.Font.Size := 12;
  FRepeat.EditLabel.Caption := '再次输入密码'; FRepeat.PasswordChar := '*';
  FCreate := Button('创建钱包',1,40,370); TWalletButton(FCreate).Primary := True;
  FResume := Button('继续备份',2,40,580); FRestore := Button('恢复钱包',7,278,370);
  FRestore.OnClick := StartRestore;
  FNext := Button('下一步：核对并设置密码',19,40,580); FNext.OnClick := RestoreStep;
  TWalletButton(FNext).Primary := True;
  FBack := Button('返回',20,40,722); FBack.OnClick := RestoreStep;
  FWordsLabel := LabelText(Self,Self,'离线抄写助记词，请勿截图或分享',40,258,460,24,10,False,Muted);
  FWords := TSecretEdit.Create(Self); FWords.Parent := Self; FWords.Multiline := True; FWords.ReadOnly := True;
  FWords.MnemonicGrid := True; FWords.TabStop := False;
  FBackupLabel := LabelText(Self,Self,'按顺序输入 24 个助记词',40,436,460,24,10,False,Muted);
  FBackup := TSecretEdit.Create(Self); FBackup.Parent := Self; FBackup.Multiline := True;
  FConfirm := Button('已抄写，进入钱包',4,40,590); TWalletButton(FConfirm).Primary := True;
  FShowAddress := Button('解锁',6,40,560); TWalletButton(FShowAddress).Primary := True;
  FExport := Button('备份助记词',18,40,614);
  FDelete := Button('删除钱包',20,278,614);
  FDone := Button('已抄写，关闭助记词',5,40,560);
  FNew := Button('创建 / 恢复其他钱包',0,40,704); FNew.OnClick := NewWallet;
  FStatus := LabelText(Self,Self,'',40,634,460,60,10,False,Muted);
  FWordCount := TComboBox.Create(Self); FWordCount.Parent := Self;
  FWordCount.Style := csDropDownList;
  for I := 0 to 4 do FWordCount.Items.Add(IntToStr(12+I*3)+' 个词');
  FWordCount.ItemIndex := 0; FWordCount.OnChange := WordCountChanged;
  for I := 0 to 23 do
  begin
    FWordNumbers[I] := LabelText(Self,Self,IntToStr(I+1)+'.',0,0,26,28,10,False,Muted);
    FWordInputs[I] := TSecretEdit.Create(Self); FWordInputs[I].Parent := Self;
  end;
  FApi := TSecureWalletApi.Create;
  Response := FApi.Request(13,[]);
  Profiles := TJSONObject.ParseJSONValue(string(Response)) as TJSONObject;
  if Profiles=nil then raise Exception.Create('钱包列表响应无效');
  try
    Names := Profiles.GetValue<TJSONArray>('profiles');
    for I := 0 to Names.Count-1 do FProfile.Items.Add(Names.Items[I].Value);
    if FProfile.Items.Count>0 then FProfile.ItemIndex := 0;
  finally Profiles.Free; Wipe(Response); end;
  FPages := TWalletDeck.Create(Self); FPages.Parent := Self; FPages.Align := alClient;
  SetupTab := FPages.AddPage; TransferTab := FPages.AddPage;
  for I := ControlCount-1 downto 0 do if Controls[I]<>FPages then Controls[I].Parent := SetupTab;
  FTransfer := TTransferPanel.Create(Self); FTransfer.Parent := TransferTab;
  FTransfer.OnManageWallet := ManageWallet;
  FTransfer.OnSelectAccount := SelectAccount;
  TransferTab.TabVisible := False; FPages.ActivePage := SetupTab;
  if SameText(ParamStr(1),'--public-preview') then
  begin
    Caption := 'Web3Rust — 公开测试预览 / 不可充值';
    FProfile.Enabled := False; FPassword.Enabled := False; FRepeat.Enabled := False; FBackup.Enabled := False;
    for I := 0 to SetupTab.ControlCount-1 do if (SetupTab.Controls[I] is TButton) then TButton(SetupTab.Controls[I]).Enabled := False;
    FTransfer.SetAccount(FApi,'public-readonly','0x0000000000000000000000000000000000000000');
    FPages.Pages[1].TabVisible := True; FPages.ActivePageIndex := 1;
  end;
  UpdateSetup;
  OnCloseQuery := Closing; FProfile.OnChange := ProfileChanged;
  FTimer := TTimer.Create(Self); FTimer.Interval := 500; FTimer.OnTimer := Tick;
  FDeadline := GetTickCount64 + IdleLockMs;
  WTSRegisterSessionNotification(Handle, 0);
  Application.OnMessage := UserInput;
end;
procedure TSetupForm.ManageWallet(Sender: TObject);
begin
  LockWallet;
end;
procedure TSetupForm.SelectAccount(Sender: TObject);
var Dialog: TAccountsForm; Choice: Integer;
begin
  if FBusy or SameText(ParamStr(1),'--public-preview') then Exit;
  FBusy := True;
  try
    if not FTransfer.BeginAccountSelection then Exit;
    try
      Dialog := TAccountsForm.CreateForWallet(Self,FApi,FProfile.Text);
      try
        Choice := Dialog.ShowModal;
        case Choice of
          mrOk: begin ShowAccount(Dialog.Address); FDeadline := GetTickCount64+IdleLockMs; end;
          mrYes: LockWallet;
          mrNo: begin
            LockWallet; FBusy := False; NewWallet(nil); FBusy := True;
          end;
          else if Dialog.Address<>'' then ShowAccount(Dialog.Address);
        end;
      finally Dialog.Free; end;
    except on E: Exception do begin LockWallet; FStatus.Caption := E.Message; end; end;
  finally FTransfer.EndAccountSelection; FBusy := False; end;
end;
destructor TSetupForm.Destroy;
var SmallIcon, LargeIcon: HICON;
begin
  Application.OnMessage := nil;
  if Assigned(FTimer) then FTimer.Enabled := False;
  if HandleAllocated then WTSUnRegisterSessionNotification(Handle);
  ClearSecrets;
  FApi.Free;
  SmallIcon := FSmallIcon; LargeIcon := FLargeIcon;
  inherited;
  if SmallIcon<>0 then DestroyIcon(SmallIcon);
  if LargeIcon<>0 then DestroyIcon(LargeIcon);
end;
procedure TSetupForm.ApplyWindowIcons;
var SmallIcon, LargeIcon, OldSmall, OldLarge: HICON;
begin
  SmallIcon := LoadImage(HInstance,'MAINICON',IMAGE_ICON,MulDiv(16,CurrentPPI,96),MulDiv(16,CurrentPPI,96),0);
  LargeIcon := LoadImage(HInstance,'MAINICON',IMAGE_ICON,MulDiv(32,CurrentPPI,96),MulDiv(32,CurrentPPI,96),0);
  if (SmallIcon=0) or (LargeIcon=0) then
  begin
    if SmallIcon<>0 then DestroyIcon(SmallIcon);
    if LargeIcon<>0 then DestroyIcon(LargeIcon);
    raise Exception.Create('Wallet icon resource unavailable');
  end;
  OldSmall := FSmallIcon; OldLarge := FLargeIcon;
  FSmallIcon := SmallIcon; FLargeIcon := LargeIcon;
  SendMessage(Handle,WM_SETICON,ICON_SMALL,FSmallIcon);
  SendMessage(Handle,WM_SETICON,ICON_BIG,FLargeIcon);
  if OldSmall<>0 then DestroyIcon(OldSmall);
  if OldLarge<>0 then DestroyIcon(OldLarge);
end;
procedure TSetupForm.CreateWnd;
begin inherited; ApplyWindowIcons; end;
procedure TSetupForm.ChangeScale(M, D: Integer; isDpiChange: Boolean);
begin
  inherited;
  if HandleAllocated then ApplyWindowIcons;
end;
procedure TSetupForm.ClearSecrets;
var I: Integer;
begin
  for I := 0 to 23 do if Assigned(FWordInputs[I]) then FWordInputs[I].Clear;
  if Assigned(FPassword) then FPassword.Clear;
  if Assigned(FRepeat) then FRepeat.Clear;
  if Assigned(FWords) then FWords.Clear;
  if Assigned(FBackup) then FBackup.Clear;
end;
procedure TSetupForm.UpdateSetup;
var Existing: Boolean; I, Count, RowHeight: Integer;
  procedure Place(C: TControl; X,Y,W,H: Integer);
  begin C.SetBounds(MulDiv(X,CurrentPPI,96),MulDiv(Y,CurrentPPI,96),MulDiv(W,CurrentPPI,96),MulDiv(H,CurrentPPI,96)); end;
begin
  Count := 12+FWordCount.ItemIndex*3;
  RowHeight := 288 div ((Count+1) div 2);
  FWordCount.Visible := FRestoreStep=1;
  Place(FWordCount,40,212,170,30); FWordCount.TabOrder := 0;
  for I := 0 to 23 do
  begin
    FWordInputs[I].Visible := (FRestoreStep=1) and (I<Count);
    FWordNumbers[I].Visible := FWordInputs[I].Visible;
    Place(FWordNumbers[I],40+(I mod 2)*238,266+(I div 2)*RowHeight,26,RowHeight-2);
    Place(FWordInputs[I],66+(I mod 2)*238,266+(I div 2)*RowHeight,196,RowHeight-2);
    FWordInputs[I].EditLabel.Visible := False;
    FWordInputs[I].TabOrder := I+1;
  end;
  FNext.TabOrder := 25; FBack.TabOrder := 26;
  FWords.EditLabel.Visible := False; FBackup.EditLabel.Visible := False;
  Existing := FProfile.Items.IndexOf(FProfile.Text)>=0;
  FNext.Visible := False; FBack.Visible := False; FNew.Visible := True;
  FRestore.Caption := '恢复钱包'; FRestore.OnClick := StartRestore;
  FPassword.EditLabel.Caption := '钱包密码';
  FProfile.Visible := not FViewingBackup; FProfileLabel.Visible := not FViewingBackup;
  FExport.Visible := Existing and not FPending and not FViewingBackup;
  FDelete.Visible := FExport.Visible and (FRestoreStep=0);
  FDone.Visible := FViewingBackup;
  Place(FExport,40,614,222,36); Place(FDelete,278,614,222,36);
  Place(FDone,40,560,460,48);
  FCreate.Visible := not Existing and not FPending; FRestore.Visible := FCreate.Visible;
  FRepeat.Visible := FCreate.Visible; FRepeat.EditLabel.Visible := FRepeat.Visible;
  FResume.Visible := FPending and FNeedsReopen; FResume.Enabled := FResume.Visible;
  FWords.Visible := FPending; FWordsLabel.Visible := FPending;
  FBackup.Visible := False; FBackupLabel.Visible := FPending;
  FConfirm.Visible := FPending; FConfirm.Enabled := FPending;
  FShowAddress.Visible := Existing and not FPending; FShowAddress.Enabled := not FPending;
  FShowAddress.Default := FShowAddress.Visible; FShowAddress.Caption := '解锁';
  FMark.Visible := Existing and not FPending;
  Place(FNew,40,722,460,40); Place(FStatus,40,660,460,52);
  if Existing and not FPending then
  begin
    FTitle.Caption := '欢迎回来'; FIntro.Caption := '解锁钱包，查看你的资产';
    Place(FTitle,40,248,460,48); Place(FIntro,40,304,460,40);
    Place(FProfileLabel,40,366,460,22); Place(FProfile,40,394,460,30);
    Place(FPassword,40,466,460,32); Place(FShowAddress,40,560,460,52);
  end
  else
  begin
    Place(FTitle,40,92,460,48); Place(FIntro,40,144,460,42);
    Place(FProfileLabel,40,196,460,22); Place(FProfile,40,222,460,30);
    if FPending then
    begin
      FTitle.Caption := '备份你的钱包'; FIntro.Caption := '妥善保管这些助记词，它们是恢复钱包的唯一凭证';
      Place(FTitle,40,80,460,48); Place(FIntro,40,132,460,40);
      FProfile.Visible := False; FProfileLabel.Visible := False;
      FBackupLabel.Caption := '按编号抄写并核对顺序，完成后点击“已抄写，进入钱包”。';
      Place(FWordsLabel,40,182,460,22); Place(FWords,40,210,460,264);
      Place(FBackupLabel,40,492,460,48);
      Place(FPassword,40,576,260,30); Place(FResume,318,568,182,44);
      Place(FConfirm,40,624,460,48); Place(FStatus,40,680,460,36);
      FPassword.Visible := FNeedsReopen; FPassword.EditLabel.Visible := FNeedsReopen;
    end
    else
    begin
      FTitle.Caption := '开启你的 Web3'; FIntro.Caption := '创建新钱包，或用助记词找回已有钱包';
      FBackupLabel.Caption := '恢复钱包时填写已有的 24 个助记词';
      Place(FPassword,40,292,460,30); Place(FRepeat,40,358,460,30);
      Place(FCreate,40,414,222,48); Place(FRestore,278,414,222,48);
      Place(FBackupLabel,40,486,460,22); Place(FBackup,40,518,460,108);
      FPassword.Visible := True; FPassword.EditLabel.Visible := True;
    end;
  end;
  if Existing and not FPending then begin FPassword.Visible := True; FPassword.EditLabel.Visible := True; end;
  if FViewingBackup then
  begin
    FTitle.Caption := '备份助记词';
    FIntro.Caption := '按顺序离线抄写这些助记词。任何获得助记词的人都能转走资产。';
    Place(FTitle,40,92,460,48); Place(FIntro,40,144,460,64);
    Place(FWordsLabel,40,230,460,22); Place(FWords,40,266,460,272);
    FWords.Visible := True; FWordsLabel.Visible := True;
    FMark.Visible := False; FShowAddress.Visible := False;
    FPassword.Visible := False; FPassword.EditLabel.Visible := False;
  end;
  if FRestoreStep>0 then
  begin
    FTitle.Caption := '恢复钱包';
    Place(FTitle,40,92,460,48); Place(FIntro,40,144,460,64);
    FMark.Visible := False; FProfile.Visible := False; FProfileLabel.Visible := False;
    FCreate.Visible := False; FExport.Visible := False; FShowAddress.Visible := False;
    FNew.Visible := False; FBack.Visible := True; Place(FBack,40,722,460,40);
    FPassword.Visible := FRestoreStep=2; FPassword.EditLabel.Visible := FPassword.Visible;
    FRepeat.Visible := FPassword.Visible; FRepeat.EditLabel.Visible := FRepeat.Visible;
    FBackup.Visible := False; FBackupLabel.Visible := False;
    FWords.Visible := FRestoreStep=2; FWordsLabel.Visible := FWords.Visible;
    FRestore.Visible := FRestoreStep=2; FNext.Visible := FRestoreStep=1;
    if FRestoreStep=1 then
    begin
      FIntro.Caption := '第 1 步 / 共 2 步：选择词数，按编号逐个填写' + #13#10 + '每格一个英文单词，按 Tab 跳到下一格';
      Place(FNext,40,580,460,48); FBack.Caption := '取消恢复';
    end else
    begin
      FIntro.Caption := '第 2 步 / 共 2 步：核对顺序，为本机钱包设置新密码';
      FWordsLabel.Caption := '请按编号核对单词顺序；有误可返回修改';
      Place(FWordsLabel,40,214,460,22); Place(FWords,40,242,460,240);
      FPassword.EditLabel.Caption := '新密码（建议至少 12 位字母或数字，无需原密码）';
      Place(FPassword,40,512,460,30); Place(FRepeat,40,580,460,30);
      Place(FRestore,40,630,460,44); Place(FStatus,40,682,460,36);
      FRestore.Caption := '恢复并进入钱包';
      FRestore.OnClick := Action; FBack.Caption := '返回修改助记词';
    end;
  end else FWordsLabel.Caption := '离线抄写助记词，请勿截图或分享';
  if SameText(ParamStr(1),'--public-preview') then
  begin
    FShowAddress.Enabled := False; FCreate.Enabled := False; FRestore.Enabled := False;
    FConfirm.Enabled := False; FResume.Enabled := False;
    FExport.Enabled := False;
  end;
end;
procedure TSetupForm.StartRestore(Sender: TObject);
begin
  if FBusy or FTransfer.Busy then Exit;
  ClearSecrets; FRestoreStep := 1; FStatus.Caption := '';
  FDeadline := GetTickCount64+IdleLockMs; UpdateSetup; FWordInputs[0].SetFocus;
end;

procedure TSetupForm.WordCountChanged(Sender: TObject);
var I: Integer;
begin
  for I := 12+FWordCount.ItemIndex*3 to 23 do FWordInputs[I].Clear;
  FBackup.Clear; FWords.Clear; FStatus.Caption := '';
  UpdateSetup;
end;

procedure TSetupForm.RestoreStep(Sender: TObject);
var Raw, Normalized: UTF8String; I, J, Count: Integer; C: AnsiChar;
begin
  if FBusy then Exit;
  if Sender=FBack then
  begin
    FPassword.Clear; FRepeat.Clear; FWords.Clear;
    Dec(FRestoreStep); FStatus.Caption := '';
    if FRestoreStep=0 then ClearSecrets;
    UpdateSetup;
    if FRestoreStep=1 then FWordInputs[0].SetFocus;
    Exit;
  end;
  try
    Count := 12+FWordCount.ItemIndex*3;
    for J := 0 to Count-1 do
    begin
      FWordInputs[J].ReadSecret(Raw,True);
      if Raw='' then
      begin
        FWordInputs[J].SetFocus;
        raise Exception.CreateFmt('请填写第 %d 个助记词。',[J+1]);
      end;
      if Length(Raw)>8 then
      begin
        FWordInputs[J].SetFocus;
        raise Exception.CreateFmt('第 %d 格只需填写一个英文单词，请检查拼写。',[J+1]);
      end;
      if J>0 then Normalized := Normalized+' ';
      for I := 1 to Length(Raw) do
      begin
        C := Raw[I];
        if (C>='A') and (C<='Z') then C := AnsiChar(Ord(C)+32);
        if not ((C>='a') and (C<='z')) then
        begin
          FWordInputs[J].SetFocus;
          raise Exception.CreateFmt('第 %d 格只填写英文单词，不需要编号、空格或标点。',[J+1]);
        end;
         SetLength(Normalized,Length(Normalized)+1); Normalized[Length(Normalized)] := C;
      end;
    end;
    FBackup.SetSecret(Normalized); FWords.SetSecret(Normalized);
    FRestoreStep := 2; FStatus.Caption := ''; UpdateSetup; FPassword.SetFocus;
  except on E: Exception do FStatus.Caption := E.Message;
  end;
  Wipe(Raw); Wipe(Normalized); C := #0;
end;
procedure TSetupForm.ShowAccount(const Value: UTF8String);
var Address: string; P, Index: Integer; Listing: UTF8String; Data: TJSONObject;
begin
  P := Pos(#10,string(Value));
  if P=0 then raise Exception.Create('地址响应不完整');
  Address := Copy(string(Value),1,P-1);
  if (Length(Address)<>42) or (Copy(Address,1,2)<>'0x') then
    raise Exception.Create('收款地址响应无效');
  Listing := FApi.Request(22,[UTF8String(FProfile.Text)]);
  Data := TJSONObject.ParseJSONValue(string(Listing)) as TJSONObject;
  try
    if Data=nil then raise Exception.Create('账户列表响应无效');
    Index := Data.GetValue<Integer>('selected');
    if Data.GetValue<TJSONArray>('addresses').Items[Index].Value<>Address then
      raise Exception.Create('当前账户与收款地址不一致');
    FTransfer.SetAccount(FApi,FProfile.Text,Address,Format('账户 %.2d',[Index+1]));
  finally Data.Free; Wipe(Listing); end;
  FPending := False; FNeedsReopen := False; FWords.Clear;
  FPages.Pages[1].TabVisible := True; FPages.ActivePageIndex := 1;
  FStatus.Caption := '';
end;
procedure TSetupForm.NewWallet(Sender: TObject);
begin
  if FBusy or FTransfer.Busy then Exit;
  FProfile.Text := 'wallet-' + FormatDateTime('yyyymmdd-hhnnss',Now) + '-' + IntToStr(GetTickCount64 mod 1000);
  ProfileChanged(nil);
  FPassword.SetFocus;
  FStatus.Caption := '新钱包名称已准备好。输入两次新密码，再点击“创建新钱包”。已有钱包文件保留不变。';
end;
procedure TSetupForm.LockWallet;
begin
  FRestoreStep := 0;
  FViewingBackup := False;
  if Assigned(FTransfer) then FTransfer.ClearAccount;
  if Assigned(FPages) then
  begin FPages.ActivePageIndex := 0; FPages.Pages[1].TabVisible := False; end;
  ClearSecrets;
  FApi.Lock;
  FNeedsReopen := FPending;
  UpdateSetup;
  FStatus.Caption := '';
  FDeadline := 0;
  FActivityPending := False;
end;
procedure TSetupForm.UserInput(var Msg: TMsg; var Handled: Boolean);
var NowTick: UInt64;
begin
  // VCL dispatches keyboard/mouse input for this app, including modal windows.
  // Timer messages and automatic balance/history requests never count as activity.
  if not (((Msg.message>=WM_KEYFIRST) and (Msg.message<=WM_KEYLAST)) or
    ((Msg.message>=WM_MOUSEFIRST) and (Msg.message<=WM_MOUSELAST)) or
    ((Msg.message>=WM_NCMOUSEMOVE) and (Msg.message<=WM_NCXBUTTONDBLCLK))) then Exit;
  NowTick := GetTickCount64;
  if (FDeadline=0) or (NowTick>=FDeadline) then Exit;
  FDeadline := NowTick+IdleLockMs;
  FActivityPending := True;
end;
procedure TSetupForm.Tick(Sender: TObject);
begin
  if not FBusy and (FDeadline <> 0) and (GetTickCount64 >= FDeadline) then
  begin
    try LockWallet; except FStatus.Caption := '会话已关闭。请重新启动程序。'; end;
  end;
  if FActivityPending and not FBusy and not FTransfer.Busy then
  begin
    FActivityPending := False;
    try FApi.Request(30,[]);
    except
      LockWallet;
      FStatus.Caption := '会话已锁定，请重新输入密码。';
    end;
  end;
end;
procedure TSetupForm.WmSessionChange(var Msg: TMessage);
begin
  if Msg.WParam = 7 then
  begin
    try LockWallet; except ClearSecrets; end;
  end;
  inherited;
end;
procedure TSetupForm.Action(Sender: TObject);
var Op, I: Integer; Password, Backup, Profile, Value: UTF8String; Addresses: TArray<string>; Address: string;
begin
  Op := TButton(Sender).Tag;
  if SameText(ParamStr(1),'--public-preview') and (Op<>5) then Exit;
  if Op = 5 then begin LockWallet; Exit; end;
  if FBusy or (Assigned(FTransfer) and FTransfer.Busy) then Exit;
  if (Op=6) and FPending then
  begin FStatus.Caption := '先完成下方备份确认，收款地址会自动显示。'; Exit; end;
  if (Op=4) and not FPending then Exit;
  if FPending and ((Op=1) or (Op=7) or ((Op=2) and not FNeedsReopen)) then Exit;
  FBusy := True; Screen.Cursor := crHourGlass;
  try
    try
      FPassword.ReadSecret(Password); Profile := UTF8String(FProfile.Text);
      FBackup.ReadSecret(Backup,True);
      if Op=4 then FWords.ReadSecret(Backup);
      if (Op = 1) or (Op = 7) then
      begin
        if Length(Password)<12 then
          raise Exception.Create('密码太短，请设置至少 12 位字母或数字的密码。');
        if not FPassword.SameSecret(FRepeat) then
          raise Exception.Create('两次密码不一致，请检查并重新输入确认密码。');
      end;
      if ((Op=2) or (Op=6) or (Op=18) or (Op=20)) and (Password='') then raise Exception.Create('请先输入上方本机密码。');
      case Op of
        20:
        begin
          if FProfile.Items.IndexOf(FProfile.Text)<0 then Exit;
          if Application.MessageBox(PChar('删除钱包“' + FProfile.Text + '”？' + #13#10#13#10 +
            '这会删除本机钱包及其所有账户的本机交易记录和缓存。' + #13#10 +
            '链上资产和已发送的交易不受影响。' + #13#10 +
            '未备份助记词将无法恢复钱包。' + #13#10#13#10 +
            '确认删除这个钱包的本机数据。'), '确认删除钱包',
            MB_YESNO or MB_ICONWARNING or MB_DEFBUTTON2)<>IDYES then Exit;
          Value := FApi.Request(21,[Profile,Password]);
          Addresses := string(Value).Split([#10]);
          for Address in Addresses do
          begin
            Backup := FApi.PublicRequest(13,[UTF8String(Address)]); Wipe(Backup);
          end;
          Wipe(Value); Wipe(Backup);
          Value := FApi.Request(20,[Profile,Password]);
          ClearSecrets; FTransfer.ClearAccount;
          FPending := False; FNeedsReopen := False; FViewingBackup := False;
          FPages.ActivePageIndex := 0; FPages.Pages[1].TabVisible := False;
          I := FProfile.Items.IndexOf(string(Profile));
          if I>=0 then FProfile.Items.Delete(I);
          if FProfile.Items.Count>0 then FProfile.ItemIndex := 0
          else FProfile.Text := 'wallet-' + FormatDateTime('yyyymmdd-hhnnss',Now);
          FStatus.Caption := '钱包及其本机记录、缓存已删除，链上资产不变。';
        end;
        18:
        begin
          FWords.Clear; FViewingBackup := False;
          Value := FApi.Request(18, [Profile, Password]);
          FWords.SetSecret(Value); FViewingBackup := True;
          FStatus.Caption := '请勿截图或分享。抄写完成后关闭；连续 2 小时无操作后自动清空。';
        end;
        1, 2: Value := FApi.Request(Op, [Profile, Password]);
        7:
        begin
          Value := FApi.Request(Op, [Profile, Password, Backup]);
          // The user already supplied and reviewed the backup; activate it once.
          FRestoreStep := 0; FPending := True; FNeedsReopen := False;
          if FProfile.Items.IndexOf(FProfile.Text)<0 then FProfile.Items.Add(FProfile.Text);
          Wipe(Value); Value := FApi.Request(4,[Backup]);
          Op := 4;
        end;
        4:
        begin
          if FNeedsReopen or (Password<>'') then
          begin
            if Password='' then raise Exception.Create('请在上方重新输入本机密码，再点击确认备份。');
            Value := FApi.Request(2,[Profile,Password]);
            if Value=#2 then
            begin
              Wipe(Value); Value := FApi.Request(6,[Profile,Password]);
            end
            else
            begin
              Wipe(Value); Wipe(Backup); Backup := FApi.Request(3,[]);
              Value := FApi.Request(4,[Backup]);
            end;
          end
          else Value := FApi.Request(4,[Backup]);
        end;
        6: Value := FApi.Request(2, [Profile, Password]);
      end;
      FPassword.Clear; FRepeat.Clear; FBackup.Clear;
      if Op=4 then for I := 0 to 23 do FWordInputs[I].Clear;
      if (Op = 1) or (Op = 2) or (Op = 7) or (Op = 6) then
      begin
        if FProfile.Items.IndexOf(FProfile.Text)<0 then FProfile.Items.Add(FProfile.Text);
        if Value = #1 then
        begin
          FPending := True; FNeedsReopen := False;
          Wipe(Value); Value := FApi.Request(3, []);
          FWords.SetSecret(Value);
          FStatus.Caption := '钱包已加密保存。抄写完成后点击“已抄写，进入钱包”；连续 2 小时无操作后自动锁定。';
        end else if Value = #2 then
        begin
          Wipe(Value); Value := FApi.Request(6,[Profile,Password]);
          ShowAccount(Value);
        end else raise Exception.Create('钱包状态响应无效');
      end
      else if (Op = 4) or (Op = 6) then
      begin
        ShowAccount(Value);
      end;
      FDeadline := GetTickCount64 + IdleLockMs;
    except
      on E: EWalletRequest do
      begin
        if (Op=4) and ((E.Code=-33) or (E.Code=-35) or (E.Code=-36)) then
        begin FNeedsReopen := True; FWords.Clear; UpdateSetup; FPassword.SetFocus; end;
        if (Op=6) and (E.Code=-37) then
        begin FPending := True; FNeedsReopen := True; end;
        FStatus.Caption := E.Message;
        if FRestoreStep=0 then begin FPassword.Clear; FRepeat.Clear; end;
        if (Op=7) and (E.Code=-31) then
          FStatus.Caption := '助记词未通过校验，请返回检查拼写及顺序，再试一次。';
      end;
      on E: Exception do
      begin
        FStatus.Caption := E.Message;
        if FRestoreStep=0 then begin FPassword.Clear; FRepeat.Clear; end;
      end;
    end;
  finally
    Wipe(Password); Wipe(Backup); Wipe(Profile); Wipe(Value);
    Screen.Cursor := crDefault; FBusy := False; UpdateSetup;
  end;
end;
procedure TSetupForm.Closing(Sender: TObject; var CanClose: Boolean);
begin
  CanClose := not (Assigned(FTransfer) and FTransfer.Busy);
  if not CanClose then LockWallet;
end;
procedure TSetupForm.ProfileChanged(Sender: TObject);
begin
  if FBusy then Exit;
  FRestoreStep := 0;
  FPending := False; FNeedsReopen := False; FViewingBackup := False; UpdateSetup;
  ClearSecrets;
  FTransfer.ClearAccount;
  FPages.ActivePageIndex := 0; FPages.Pages[1].TabVisible := False;
  FApi.Lock;
  FStatus.Caption := '';
end;
procedure TSetupForm.RunSelfTest;
var Ppi: Integer; Solana: TSolanaForm; Tron: TTronForm; Input: TMsg; Handled: Boolean; Previous: UInt64;
begin
  if (FPages.ActivePageIndex<>0) or FPages.Pages[1].TabVisible then raise Exception.Create('Initial wallet setup routing failed');
  Left := -10000; Show; Update;
  FPassword.RunBufferSelfTest;
  FTransfer.RunAmountSelfTest;
  FPages.ActivePageIndex := 1; FTransfer.RunNetworkNavigationSelfTest; FPages.ActivePageIndex := 0;
  FPassword.SetSecret('PUBLIC TEST ONLY'); FWords.SetSecret('PUBLIC TEST ONLY'); FBackup.SetSecret('PUBLIC TEST ONLY');
  LockWallet;
  FPassword.CheckErased; FRepeat.CheckErased; FWords.CheckErased; FBackup.CheckErased;
  if not FPassword.IsEmpty or not FWords.IsEmpty or not FBackup.IsEmpty then raise Exception.Create('Lock display clear failed');
  if IdleLockMs<>7200000 then raise Exception.Create('Idle lock duration mismatch');
  Input := Default(TMsg); Handled := False;
  FDeadline := GetTickCount64+1000; Previous := FDeadline;
  Input.message := WM_TIMER; UserInput(Input,Handled);
  if FDeadline<>Previous then raise Exception.Create('Background timer renewed idle lock');
  Input.message := WM_KEYDOWN; UserInput(Input,Handled);
  if FDeadline<GetTickCount64+7199000 then raise Exception.Create('Keyboard did not renew two-hour idle deadline');
  Tick(nil);
  if FActivityPending then raise Exception.Create('Activity was not sent to contained signer');
  FDeadline := GetTickCount64+1000;
  Input.message := WM_MOUSEMOVE; UserInput(Input,Handled);
  if FDeadline<GetTickCount64+7199000 then raise Exception.Create('Mouse did not renew idle deadline');
  FDeadline := GetTickCount64; Previous := FDeadline; UserInput(Input,Handled);
  if FDeadline<>Previous then raise Exception.Create('Input revived expired idle deadline');
  Tick(nil);
  if FDeadline<>0 then raise Exception.Create('Idle expiration did not lock');
  for Ppi in [96,144,96] do
  begin
    ScaleForPPI(Ppi); ClientWidth := MulDiv(540,Ppi,96); ClientHeight := MulDiv(800,Ppi,96); UpdateSetup;
    FPages.ActivePageIndex := 1; FTransfer.CheckLayout; FPages.ActivePageIndex := 0;
    if FNew.Top+FNew.Height>FPages.Pages[0].ClientHeight then raise Exception.Create('Setup layout clipped');
  end;
  RunBackupFlowSelfTest;
  Solana := TSolanaForm.Create(Self);
  try Solana.RunSelfTest; finally Solana.Free; end;
  Tron := TTronForm.Create(Self);
  try Tron.RunSelfTest; finally Tron.Free; end;
  Hide;
end;
procedure TSetupForm.RunBackupFlowSelfTest;
var SavedWords, AccountValue, SecondAccount: UTF8String; Deadline: UInt64; I: Integer; Accounts: TAccountsForm;
  function WordsMatch: Boolean;
  var Current: UTF8String;
  begin
    try FWords.ReadSecret(Current); Result := Current=SavedWords; finally Wipe(Current); end;
  end;
  procedure WaitForRefresh;
  begin
    Deadline := GetTickCount64+55000;
    while FTransfer.Busy and (GetTickCount64<Deadline) do begin Application.ProcessMessages; Sleep(5); end;
    if FTransfer.Busy then raise Exception.Create('GUI receive refresh timeout');
  end;
begin
  // This runs only in the disposable test container; no words or addresses are logged.
  if not SameText(ParamStr(1),'--network-self-test') and not SameText(ParamStr(1),'--self-test') then
    raise Exception.Create('Test mode required');
  Left := -10000; Show; Update;
  NewWallet(nil);
  FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); FRepeat.SetSecret('DISPOSABLE TEST PASSWORD');
  Action(FCreate);
  if not FPending or FShowAddress.Enabled or FWords.IsEmpty then raise Exception.Create('GUI create/backup state failed');
  FWords.ReadSecret(SavedWords);
  try
    Action(FShowAddress);
    if not WordsMatch then raise Exception.Create('Premature address action destroyed backup');
    // Reopening a pending wallet uses the same entry point as an active wallet.
    ProfileChanged(nil);
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FShowAddress);
    if not FPending or (not WordsMatch) or not FConfirm.Visible then
      raise Exception.Create('Existing pending wallet routing failed');
    if FBackup.Visible then raise Exception.Create('Backup must not require mnemonic re-entry');
    // Simulate a disconnected/expired sensitive session with the same visible UI.
    FApi.Lock;
    Action(FConfirm);
    if not FNeedsReopen or not FPending or not FWords.IsEmpty then raise Exception.Create('Lost backup session recovery failed');
    FPassword.SetSecret('WRONG TEST PASSWORD'); Action(FConfirm);
    if not FNeedsReopen or (Pos('-32',FStatus.Caption)=0) then raise Exception.Create('Recovery password rejection failed');
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FConfirm);
    if FPending or (FPages.ActivePageIndex<>1) or not FWords.IsEmpty or not FBackup.IsEmpty then raise Exception.Create('GUI backup activation/receive transition failed');
    WaitForRefresh;
    // Repeated confirmation after activation cannot re-enter the pending flow.
    Action(FConfirm);
    if FPending then raise Exception.Create('Repeated confirmation regression');
    LockWallet;
    if (FPages.ActivePageIndex<>0) or FPages.Pages[1].TabVisible or FBackup.Visible or FRepeat.Visible then
      raise Exception.Create('Active wallet lock/setup routing failed');
    FPassword.SetSecret('WRONG TEST PASSWORD'); Action(FShowAddress);
    if (FPages.ActivePageIndex<>0) or FPages.Pages[1].TabVisible or (Pos('-32',FStatus.Caption)=0) then
      raise Exception.Create('Existing wallet wrong password routing failed');
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FShowAddress);
    if FPending or (FPages.ActivePageIndex<>1) or not FPassword.IsEmpty or FBackup.Visible then
      raise Exception.Create('Existing active wallet direct open failed');
    WaitForRefresh;
    LockWallet;
    // Regression: the old resume button must also open an already active wallet directly.
    FPassword.SetSecret('WRONG TEST PASSWORD'); Action(FExport);
    if FViewingBackup or not FWords.IsEmpty or (Pos('-32',FStatus.Caption)=0) then
      raise Exception.Create('Backup password rejection failed');
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FExport);
    if not FViewingBackup or not WordsMatch or not FPassword.IsEmpty or not FDone.Visible then
      raise Exception.Create('Active wallet backup display failed');
    Action(FDone);
    FWords.CheckErased;
    if FViewingBackup or not FWords.IsEmpty then raise Exception.Create('Backup close failed');
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FExport);
    FDeadline := GetTickCount64; Tick(nil);
    FWords.CheckErased;
    if FViewingBackup then raise Exception.Create('Backup timeout failed');
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FExport);
    ProfileChanged(nil); FWords.CheckErased;
    if FViewingBackup then raise Exception.Create('Backup profile switch failed');
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FResume);
    if FPending or (FPages.ActivePageIndex<>1) then raise Exception.Create('Active wallet resume regression');
    WaitForRefresh;
    LockWallet;
    Wipe(SavedWords);
    SavedWords := 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
    NewWallet(nil); StartRestore(nil);
    FWordCount.ItemIndex := 4; WordCountChanged(nil);
    if not FWordInputs[23].Visible then raise Exception.Create('24 word input layout failed');
    FWordInputs[23].SetSecret('test');
    FWordCount.ItemIndex := 0; WordCountChanged(nil);
    FWordInputs[23].CheckErased;
    RestoreStep(FNext);
    if (FRestoreStep<>1) or (ActiveControl<>FWordInputs[0]) then raise Exception.Create('Restore missing word focus failed');
    for I := 0 to 10 do FWordInputs[I].SetSecret('abandon');
    FWordInputs[11].SetSecret('about'); RestoreStep(FNext);
    if (FRestoreStep<>2) or not WordsMatch then raise Exception.Create('Restore review failed');
    RestoreStep(FBack);
    if (FRestoreStep<>1) or FBackup.IsEmpty then raise Exception.Create('Restore back lost input');
    RestoreStep(FNext);
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); FRepeat.SetSecret('DISPOSABLE TEST PASSWORD');
    Action(FRestore);
    if FPending or (FRestoreStep<>0) or (FPages.ActivePageIndex<>1) or not FBackup.IsEmpty then
      raise Exception.Create('Restore direct activation failed');
    for I := 0 to 23 do FWordInputs[I].CheckErased;
    WaitForRefresh; LockWallet;
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FExport);
    if not WordsMatch then raise Exception.Create('Restored wallet mnemonic mismatch');
    LockWallet;
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FShowAddress);
    WaitForRefresh;
    if not FTransfer.BeginAccountSelection then raise Exception.Create('Account selector busy');
    try
      Accounts := TAccountsForm.CreateForWallet(Self,FApi,FProfile.Text);
      try Accounts.RunSelfTest; finally Accounts.Free; end;
      SecondAccount := FApi.Request(24,[UTF8String(FProfile.Text),'1']);
      ShowAccount(SecondAccount);
    finally FTransfer.EndAccountSelection; end;
    WaitForRefresh; LockWallet;
    FPassword.SetSecret('DISPOSABLE TEST PASSWORD'); Action(FShowAddress);
    WaitForRefresh;
    AccountValue := FApi.Request(24,[UTF8String(FProfile.Text),'1']);
    if AccountValue<>SecondAccount then raise Exception.Create('Second account restart changed address');
    Wipe(AccountValue); Wipe(SecondAccount); LockWallet;
  finally Wipe(SavedWords); Hide; end;
end;
procedure TSetupForm.RunTronNetworkSelfTest;
var Value: UTF8String; Data: TJSONObject;
begin
  Value := FApi.PublicRequest(18,['TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or (Data.GetValue<string>('network')<>'tron-mainnet') or
      (Data.GetValue<Integer>('usdt_decimals')<>6) or
      (Data.GetValue<string>('contract')<>'TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t') then
      raise Exception.Create('Tron isolated public balance identity failed');
  finally Data.Free; Wipe(Value); end;
end;
procedure TSetupForm.RunNetworkSelfTest;
var Value: UTF8String; Data: TJSONObject;
begin
  RunBackupFlowSelfTest;
  RunTronNetworkSelfTest;
  Value := FApi.PublicRequest(14,['DYxjbWgTSajxKH1Fwcr6hrvXJ3ww9KdtG6jT2qkqD4YS']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or (Data.GetValue<string>('network')<>'solana-mainnet') or
      (Data.GetValue<Integer>('usdt_decimals')<>6) or
      (Data.GetValue<string>('mint')<>'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB') then
      raise Exception.Create('Solana isolated public balance identity failed');
  finally Data.Free; Wipe(Value); end;
  Value := FApi.PublicRequest(13,['0x0000000000000000000000000000000000000001']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or not Data.GetValue<Boolean>('deleted') then
      raise Exception.Create('Local cache deletion through broker failed');
  finally Data.Free; Wipe(Value); end;
  Value := FApi.PublicRequest(8,['0x0000000000000000000000000000000000000001','']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or (Data.GetValue<TJSONArray>('entries').Count<>0) then
      raise Exception.Create('Deleted cache still contains records');
  finally Data.Free; Wipe(Value); end;
  FTransfer.RunReadOnlySelfTest(FApi);
  Value := FApi.PublicRequest(1, ['0x0000000000000000000000000000000000000000']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data = nil) or (Data.GetValue<Integer>('chain_id') <> 56) or
      (Data.GetValue<string>('asset') <> 'BNB') then raise Exception.Create('Public balance identity mismatch');
  finally Data.Free; end;
  Value := FApi.PublicRequest(4, [UTF8String(StringOfChar('0', 64))]);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data = nil) or (Data.GetValue<string>('state') <> 'unknown') then raise Exception.Create('Public unknown transaction mismatch');
  finally Data.Free; end;
  Value := FApi.PublicRequest(7,['0x0000000000000000000000000000000000000001','']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or (Data.GetValue<Integer>('chain_id')<>56) or
      (Data.GetValue<string>('asset')<>'USDT') or not Data.GetValue<Boolean>('finalized') or
      (Data.GetValue<TJSONArray>('entries').Count>3) then raise Exception.Create('Public chain history response failed');
  finally Data.Free; end;
  Value := FApi.PublicRequest(8,['0x0000000000000000000000000000000000000001','']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or not Data.GetValue<Boolean>('cached') or
      (Data.GetValue<Integer>('chain_id')<>56) then raise Exception.Create('Persistent history cache response failed');
  finally Data.Free; end;
  Value := FApi.PublicRequest(7,['0x0000000000000000000000000000000000000001','']);
  Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if (Data=nil) or Data.GetValue<Boolean>('cached') then raise Exception.Create('Incremental history refresh failed');
  finally Data.Free; end;
end;
end.
