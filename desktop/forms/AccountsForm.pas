unit AccountsForm;
interface
uses Winapi.Windows, Winapi.Messages, System.SysUtils, System.Classes, System.JSON, System.Types,
  Vcl.Forms, Vcl.Controls, Vcl.StdCtrls, Vcl.ExtCtrls, Vcl.Graphics, WalletTheme, SecureWalletApi;
type
  TAccountsForm = class(TForm)
  private
    FApi: TSecureWalletApi;
    FProfile: UTF8String;
    FList: TScrollBox;
    FAdd: TButton;
    FStatus: TLabel;
    FAddress: UTF8String;
    FSelected, FCount: Integer;
    procedure Choose(Sender: TObject);
    procedure AddAccount(Sender: TObject);
    procedure LoadAccounts;
    procedure Manage(Sender: TObject);
    procedure NewWallet(Sender: TObject);
  public
    constructor CreateForWallet(AOwner: TComponent; Api: TSecureWalletApi; const Profile: string);
    property Address: UTF8String read FAddress;
    procedure RunSelfTest;
  end;
implementation
uses Vcl.Imaging.pngimage;
constructor TAccountsForm.CreateForWallet(AOwner: TComponent; Api: TSecureWalletApi; const Profile: string);
var Header, Footer: TPanel; B: TButton; L: TLabel;
begin
  inherited CreateNew(AOwner);
  FApi := Api; FProfile := UTF8String(Profile);
  Caption := '钱包与账户'; ClientWidth := 500; ClientHeight := 680;
  BorderStyle := bsSingle; BorderIcons := [biSystemMenu]; Position := poOwnerFormCenter;
  Font.Name := 'Microsoft YaHei UI'; Font.Size := 10; Font.Color := Ink; Color := clWhite;
  Header := TPanel.Create(Self); Header.Parent := Self; Header.Align := alTop;
  Header.Height := 138; Header.BevelOuter := bvNone; Header.Color := clWhite; Header.ParentBackground := False;
  LabelText(Self,Header,'钱包与账户',24,18,452,36,22,True);
  L := LabelText(Self,Header,Profile,24,64,452,28,12,True);
  L.WordWrap := False; L.EllipsisPosition := epEndEllipsis;
  LabelText(Self,Header,'同一组助记词 · 每个账户都有独立地址',24,102,452,24,10,False,Muted);
  Footer := TPanel.Create(Self); Footer.Parent := Self; Footer.Align := alBottom;
  Footer.Height := 158; Footer.BevelOuter := bvNone; Footer.Color := clWhite; Footer.ParentBackground := False;
  FStatus := LabelText(Self,Footer,'',24,8,452,48,9,False,Muted);
  FAdd := TWalletButton.Create(Self); FAdd.Parent := Footer; FAdd.SetBounds(24,60,452,38);
  FAdd.Caption := '+ 添加账户'; FAdd.OnClick := AddAccount;
  B := TWalletButton.Create(Self); B.Parent := Footer; B.SetBounds(24,110,218,38);
  B.Caption := '管理钱包'; B.OnClick := Manage;
  B := TWalletButton.Create(Self); B.Parent := Footer; B.SetBounds(258,110,218,38);
  B.Caption := '添加钱包'; B.OnClick := NewWallet;
  FList := TScrollBox.Create(Self); FList.Parent := Self; FList.Align := alClient;
  FList.BorderStyle := bsNone; FList.HorzScrollBar.Visible := False;
  LoadAccounts;
end;
procedure TAccountsForm.LoadAccounts;
var Value: UTF8String; Data: TJSONObject; Items: TJSONArray; I: Integer; B: TButton; A: string;
begin
  Value := FApi.Request(22,[FProfile]); Data := TJSONObject.ParseJSONValue(string(Value)) as TJSONObject;
  try
    if Data=nil then raise Exception.Create('账户列表响应无效');
    Items := Data.GetValue<TJSONArray>('addresses'); FSelected := Data.GetValue<Integer>('selected');
    FCount := Items.Count;
    if (FCount<1) or (FCount>32) or (FSelected<0) or (FSelected>=FCount) then
      raise Exception.Create('账户列表响应无效');
    while FList.ControlCount>0 do FList.Controls[0].Free;
    for I := 0 to FCount-1 do
    begin
      A := Items.Items[I].Value;
      B := TWalletButton.Create(Self); B.Parent := FList;
      B.SetBounds(MulDiv(24,CurrentPPI,96),MulDiv(10+I*64,CurrentPPI,96),
        FList.ClientWidth-MulDiv(48,CurrentPPI,96),MulDiv(56,CurrentPPI,96));
      B.Anchors := [akLeft,akTop,akRight]; B.Tag := I; B.OnClick := Choose;
      B.Caption := Format('账户 %.2d   %s…%s',[I+1,Copy(A,1,8),Copy(A,Length(A)-5,6)]);
      if I=FSelected then begin B.Caption := '✓  '+B.Caption; TWalletButton(B).Primary := True; end;
      B.Hint := A; B.ShowHint := True;
    end;
    FAdd.Enabled := FCount<Data.GetValue<Integer>('limit');
    FStatus.Caption := Format('已添加 %d 个账户。选择账户可查看各自的余额和记录。',[FCount]);
    if not FAdd.Enabled then FStatus.Caption := '已达到每个钱包 32 个账户的上限。';
  finally Data.Free; Wipe(Value); end;
end;
procedure TAccountsForm.Choose(Sender: TObject);
begin
  try
    FAddress := FApi.Request(24,[FProfile,UTF8String(IntToStr(TButton(Sender).Tag))]);
    ModalResult := mrOk;
  except on E: Exception do FStatus.Caption := E.Message; end;
end;
procedure TAccountsForm.AddAccount(Sender: TObject);
var Previous: Integer;
begin
  Previous := FSelected;
  FAdd.Enabled := False;
  try
    FAddress := FApi.Request(23,[FProfile]);
    // Adding stays in this list; only choosing a row switches the account.
    FAddress := FApi.Request(24,[FProfile,UTF8String(IntToStr(Previous))]);
    LoadAccounts;
    FList.VertScrollBar.Position := FList.VertScrollBar.Range;
  except on E: Exception do begin FStatus.Caption := E.Message; FAdd.Enabled := FCount<32; end; end;
end;
procedure TAccountsForm.Manage(Sender: TObject);
begin ModalResult := mrYes; end;
procedure TAccountsForm.NewWallet(Sender: TObject);
begin ModalResult := mrNo; end;
procedure TAccountsForm.RunSelfTest;
var Original, Selected, Ppi: Integer; Bitmap: TBitmap; Png: TPngImage; I, SavedDC: Integer;
    B: TWalletButton; Point: TPoint; Draw: TDrawItemStruct;
begin
  Original := FCount; Selected := FSelected; AddAccount(FAdd);
  if (ModalResult<>mrNone) or (Pos(#10,string(FAddress))=0) then raise Exception.Create('Adding closed account window');
  if (FCount<>Original+1) or (FSelected<>Selected) then raise Exception.Create('Adding changed selection or failed to refresh');
  AddAccount(FAdd);
  if (ModalResult<>mrNone) or (FCount<>Original+2) or (FSelected<>Selected) then
    raise Exception.Create('Consecutive account creation failed');
  for Ppi in [96,144,96] do
  begin
    ScaleForPPI(Ppi); LoadAccounts;
    for I := 0 to FList.ControlCount-1 do
      if FList.Controls[I].Left+FList.Controls[I].Width>FList.ClientWidth then
        raise Exception.Create('Account row layout clipped');
  end;
  if SameText(ParamStr(1),'--self-test') then
  begin
    Left := -10000; Show; Update;
    Bitmap := TBitmap.Create; Png := TPngImage.Create;
    try
      Bitmap.SetSize(ClientWidth,ClientHeight); PaintTo(Bitmap.Canvas.Handle,0,0);
      // BS_OWNERDRAW buttons do not implement WM_PRINTCLIENT; invoke their
      // normal draw handler against the test bitmap at the actual bounds.
      for I := 0 to ComponentCount-1 do
        if Components[I] is TWalletButton then
        begin
          B := TWalletButton(Components[I]);
          Point := ScreenToClient(B.ClientToScreen(System.Types.Point(0,0)));
          SavedDC := SaveDC(Bitmap.Canvas.Handle);
          try
            SetViewportOrgEx(Bitmap.Canvas.Handle,Point.X,Point.Y,nil);
            FillChar(Draw,SizeOf(Draw),0); Draw.hDC := Bitmap.Canvas.Handle;
            Draw.rcItem := B.ClientRect; Draw.hwndItem := B.Handle; Draw.CtlType := ODT_BUTTON;
            B.Perform(CN_DRAWITEM,0,LPARAM(@Draw));
          finally RestoreDC(Bitmap.Canvas.Handle,SavedDC); end;
        end;
      Png.Assign(Bitmap); Png.SaveToFile(IncludeTrailingPathDelimiter(GetEnvironmentVariable('TEMP'))+'accounts-preview.png');
    finally Png.Free; Bitmap.Free; Hide; end;
  end;
  Choose(FList.Controls[0]); LoadAccounts;
  if (FSelected<>0) or (ModalResult<>mrOk) then raise Exception.Create('Account selection UI failed');
end;
end.
